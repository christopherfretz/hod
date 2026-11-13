//! Attributes (`spec/ir.md` §5.7): the calling convention, the attributes
//! of one parameter or return value, the attributes of a function, and the
//! table that puts every LLVM attribute name in one of three classes.
//!
//! Only attributes that change the generated code are representable.
//! [`CallConv`] and [`ParamAttrs`] are shared by function symbols (§2.2)
//! and call instructions (§5.4). [`FnAttrs`] is the set of kept
//! function-level attributes; sets repeat across the functions of a
//! module, so a module interns them and a function symbol holds a
//! [`FnAttrsId`]. [`attr_class`] is the single place that says whether an
//! attribute is kept, dropped or rejected; producers and `classify` both
//! read it.

use std::fmt;

use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::TypeId;
use crate::intern::{Hasher64, InternTable, Layered, Layering, NoLayer, Seg, next_id, pool_start};
use crate::strings::{StrId, StrPool};

/// A calling convention, by LLVM's number (`llvm::CallingConv::ID`).
///
/// Every number LLVM accepts (0 to 1023) is representable, so a producer
/// never has to refuse one; `classify` (§8) decides which are supported and
/// names the others `cc:<name>`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct CallConv(u16);

impl CallConv {
    /// `ccc`, the default.
    pub const C: CallConv = CallConv(0);
    /// `fastcc`.
    pub const FAST: CallConv = CallConv(8);
    /// `coldcc`.
    pub const COLD: CallConv = CallConv(9);
    /// `ghccc`.
    pub const GHC: CallConv = CallConv(10);
    /// `anyregcc`.
    pub const ANY_REG: CallConv = CallConv(13);
    /// `preserve_mostcc`.
    pub const PRESERVE_MOST: CallConv = CallConv(14);
    /// `preserve_allcc`.
    pub const PRESERVE_ALL: CallConv = CallConv(15);
    /// `swiftcc`.
    pub const SWIFT: CallConv = CallConv(16);
    /// `cxx_fast_tlscc`.
    pub const CXX_FAST_TLS: CallConv = CallConv(17);
    /// `tailcc`.
    pub const TAIL: CallConv = CallConv(18);
    /// `cfguard_checkcc`.
    pub const CFGUARD_CHECK: CallConv = CallConv(19);
    /// `swifttailcc`.
    pub const SWIFT_TAIL: CallConv = CallConv(20);
    /// `preserve_nonecc`.
    pub const PRESERVE_NONE: CallConv = CallConv(21);
    /// `aarch64_vector_pcs`.
    pub const AARCH64_VECTOR_PCS: CallConv = CallConv(97);
    /// `aarch64_sve_vector_pcs`.
    pub const AARCH64_SVE_VECTOR_PCS: CallConv = CallConv(98);

    /// The largest number LLVM accepts (`CallingConv::MaxID`).
    pub const MAX: u16 = 1023;

    /// The convention with LLVM number `id`.
    ///
    /// # Errors
    /// `InvalidOperand` if `id` is above [`CallConv::MAX`].
    pub fn from_llvm(id: u32) -> IrResult<CallConv> {
        match u16::try_from(id) {
            Ok(n) if n <= Self::MAX => Ok(CallConv(n)),
            _ => err(IrErrorKind::InvalidOperand, format!("calling convention {id} is above 1023")),
        }
    }

    /// LLVM's number.
    #[inline]
    pub const fn llvm_id(self) -> u16 {
        self.0
    }

    /// The keyword LLVM's text uses, or `None` for a convention this table
    /// does not name (LLVM prints those as `cc N`).
    pub const fn keyword(self) -> Option<&'static str> {
        Some(match self.0 {
            0 => "ccc",
            8 => "fastcc",
            9 => "coldcc",
            10 => "ghccc",
            13 => "anyregcc",
            14 => "preserve_mostcc",
            15 => "preserve_allcc",
            16 => "swiftcc",
            17 => "cxx_fast_tlscc",
            18 => "tailcc",
            19 => "cfguard_checkcc",
            20 => "swifttailcc",
            21 => "preserve_nonecc",
            97 => "aarch64_vector_pcs",
            98 => "aarch64_sve_vector_pcs",
            _ => return None,
        })
    }
}

impl Default for CallConv {
    fn default() -> Self {
        CallConv::C
    }
}

/// The attribute of a parameter that carries a type. A parameter has at
/// most one.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ParamMem {
    /// None of them.
    None,
    /// `sret(T)`: the pointer to the caller's storage for the return value.
    Sret,
    /// `byval(T)`: the callee receives a copy of the pointee, made by the
    /// caller on the stack.
    ByVal,
    /// `byref(T)`.
    ByRef,
    /// `inalloca(T)`.
    InAlloca,
    /// `preallocated(T)`.
    Preallocated,
    /// `elementtype(T)`: the pointee type of an indirect inline-assembly
    /// operand or an intrinsic argument (call sites only).
    ElementType,
}

/// The code-affecting attributes of one parameter, or of the return value
/// (§5.7, "kept ... on parameters and returns"). Eight bytes, `Copy`.
///
/// Built from [`ParamAttrs::NONE`] with the `with_*` methods:
///
/// ```
/// use hod_ir::{ParamAttrs, ParamMem, TypeId};
/// let a = ParamAttrs::NONE.with_mem(ParamMem::ByVal, TypeId::I64)?.with_align(16)?;
/// assert_eq!(a.mem(), ParamMem::ByVal);
/// assert_eq!(a.mem_type(), Some(TypeId::I64));
/// assert_eq!(a.align(), Some(16));
/// # Ok::<(), hod_ir::IrError>(())
/// ```
///
/// What is checked here is what the representation cannot hold: two typed
/// attributes on one parameter, and `zeroext` with `signext`. The other
/// combinations LLVM's verifier refuses (`byval` with `nest`, ...) are
/// representable; a producer of unverified IR that needs them refused
/// checks for itself.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ParamAttrs {
    /// Flag bits (`ZEROEXT`..) and, in the low three bits, the `ParamMem`.
    bits: u16,
    /// `align N`: 0 for none, else log2(N) + 1.
    align: u8,
    /// `alignstack(N)`: 0 for none, else log2(N) + 1.
    stack_align: u8,
    /// The type of the `ParamMem` attribute; `VOID` when there is none.
    mem_ty: TypeId,
}

const MEM_MASK: u16 = 0b111;

impl ParamAttrs {
    /// No attributes.
    pub const NONE: ParamAttrs =
        ParamAttrs { bits: 0, align: 0, stack_align: 0, mem_ty: TypeId::VOID };

    /// `zeroext`.
    pub const ZEROEXT: u16 = 1 << 3;
    /// `signext`.
    pub const SIGNEXT: u16 = 1 << 4;
    /// `inreg`.
    pub const INREG: u16 = 1 << 5;
    /// `nest`.
    pub const NEST: u16 = 1 << 6;
    /// `swiftself`.
    pub const SWIFT_SELF: u16 = 1 << 7;
    /// `swifterror`.
    pub const SWIFT_ERROR: u16 = 1 << 8;
    /// `swiftasync`.
    pub const SWIFT_ASYNC: u16 = 1 << 9;

    const ALL_FLAGS: u16 = Self::ZEROEXT
        | Self::SIGNEXT
        | Self::INREG
        | Self::NEST
        | Self::SWIFT_SELF
        | Self::SWIFT_ERROR
        | Self::SWIFT_ASYNC;

    /// These attributes plus the flags in `flags` (an OR of the flag
    /// constants).
    ///
    /// # Errors
    /// `InvalidOperand` for a bit that is not a flag constant, and for
    /// `zeroext` together with `signext`.
    pub fn with(self, flags: u16) -> IrResult<ParamAttrs> {
        if flags & !Self::ALL_FLAGS != 0 {
            return err(
                IrErrorKind::InvalidOperand,
                format!("{flags:#x} is not a set of parameter flags"),
            );
        }
        let bits = self.bits | flags;
        if bits & Self::ZEROEXT != 0 && bits & Self::SIGNEXT != 0 {
            return err(IrErrorKind::InvalidOperand, "zeroext and signext on one parameter");
        }
        Ok(ParamAttrs { bits, ..self })
    }

    /// These attributes plus the typed attribute `mem` with type `ty`.
    ///
    /// # Errors
    /// `InvalidOperand` if `mem` is `ParamMem::None`, or the parameter
    /// already has a typed attribute (LLVM allows `elementtype` beside
    /// another one on an inline-assembly operand; that combination is not
    /// representable).
    /// The same attributes with the memory type translated
    /// (`spec/ir.md` §5.1).
    pub(crate) fn map_mem_ty(self, f: impl FnOnce(TypeId) -> TypeId) -> ParamAttrs {
        ParamAttrs { mem_ty: f(self.mem_ty), ..self }
    }

    pub fn with_mem(self, mem: ParamMem, ty: TypeId) -> IrResult<ParamAttrs> {
        if mem == ParamMem::None {
            return err(IrErrorKind::InvalidOperand, "a typed parameter attribute needs a kind");
        }
        if self.mem() != ParamMem::None {
            return err(
                IrErrorKind::InvalidOperand,
                format!("{:?} and {mem:?} on one parameter", self.mem()),
            );
        }
        Ok(ParamAttrs { bits: self.bits | mem as u16, mem_ty: ty, ..self })
    }

    /// These attributes plus `align bytes`.
    ///
    /// # Errors
    /// `InvalidOperand` unless `bytes` is a power of two up to 2^32.
    pub fn with_align(self, bytes: u64) -> IrResult<ParamAttrs> {
        Ok(ParamAttrs { align: encode_align(bytes)?, ..self })
    }

    /// These attributes plus `alignstack(bytes)`.
    ///
    /// # Errors
    /// `InvalidOperand` unless `bytes` is a power of two up to 2^32.
    pub fn with_stack_align(self, bytes: u64) -> IrResult<ParamAttrs> {
        Ok(ParamAttrs { stack_align: encode_align(bytes)?, ..self })
    }

    /// True if no attribute is set.
    #[inline]
    pub fn is_none(self) -> bool {
        self == Self::NONE
    }

    /// True if every flag in `flags` is set.
    #[inline]
    pub const fn has(self, flags: u16) -> bool {
        self.bits & flags & Self::ALL_FLAGS == flags
    }

    /// The typed attribute.
    #[inline]
    pub const fn mem(self) -> ParamMem {
        match self.bits & MEM_MASK {
            1 => ParamMem::Sret,
            2 => ParamMem::ByVal,
            3 => ParamMem::ByRef,
            4 => ParamMem::InAlloca,
            5 => ParamMem::Preallocated,
            6 => ParamMem::ElementType,
            _ => ParamMem::None,
        }
    }

    /// The typed attribute's type.
    #[inline]
    pub fn mem_type(self) -> Option<TypeId> {
        if self.bits & MEM_MASK == 0 { None } else { Some(self.mem_ty) }
    }

    /// `align N`, in bytes.
    #[inline]
    pub const fn align(self) -> Option<u64> {
        decode_align(self.align)
    }

    /// `alignstack(N)`, in bytes.
    #[inline]
    pub const fn stack_align(self) -> Option<u64> {
        decode_align(self.stack_align)
    }

    /// All fields in one word, for hashing.
    #[inline]
    fn packed(self) -> u64 {
        u64::from(self.bits)
            | u64::from(self.align) << 16
            | u64::from(self.stack_align) << 24
            | u64::from(self.mem_ty.raw()) << 32
    }
}

impl Default for ParamAttrs {
    fn default() -> Self {
        ParamAttrs::NONE
    }
}

pub(crate) fn encode_align(bytes: u64) -> IrResult<u8> {
    if !bytes.is_power_of_two() || bytes > 1 << 32 {
        return err(
            IrErrorKind::InvalidOperand,
            format!("alignment {bytes} is not a power of two up to 2^32"),
        );
    }
    // A power of two up to 2^32 has at most 32 trailing zeros.
    Ok(u8::try_from(bytes.trailing_zeros()).unwrap_or(32) + 1)
}

pub(crate) const fn decode_align(code: u8) -> Option<u64> {
    if code == 0 { None } else { Some(1 << (code - 1)) }
}

// ---- parameter attribute lists ----

/// The interned `[return, parameter 0, parameter 1, ..]` attribute lists
/// of a module's functions. Trailing `NONE`s are not stored, so the list
/// of a function without parameter attributes is the empty list, id 0.
#[derive(Clone, Debug)]
pub(crate) struct SigAttrTable {
    /// `(start, len)` in `pool`.
    lists: Vec<(u32, u32)>,
    pool: Vec<ParamAttrs>,
    interner: InternTable,
}

impl Default for SigAttrTable {
    fn default() -> Self {
        SigAttrTable { lists: vec![(0, 0)], pool: Vec::new(), interner: InternTable::new() }
    }
}

impl SigAttrTable {
    /// The id of the empty list.
    pub(crate) const EMPTY: u32 = 0;

    /// The id of the list `ret, params..` with trailing `NONE`s removed.
    pub(crate) fn intern(&mut self, ret: ParamAttrs, params: &[ParamAttrs]) -> IrResult<u32> {
        let used = params.iter().rposition(|p| !p.is_none()).map_or(0, |last| last + 1);
        let params = &params[..used];
        if params.is_empty() && ret.is_none() {
            return Ok(Self::EMPTY);
        }
        let mut h = Hasher64::new(params.len() as u64);
        h.word(ret.packed());
        for p in params {
            h.word(p.packed());
        }
        let hash = h.finish();
        let same = |id: u32| {
            let list = self.get(id);
            list.len() == params.len() + 1 && list[0] == ret && list[1..] == *params
        };
        if let Some(found) = self.interner.find(hash, same) {
            return Ok(found);
        }
        let id = next_id(self.lists.len(), "parameter attribute list")?;
        let start = pool_start(self.pool.len(), params.len() + 1, "parameter attribute")?;
        self.pool.push(ret);
        self.pool.extend_from_slice(params);
        self.lists.push((start, params.len() as u32 + 1));
        self.interner.insert(hash, id);
        Ok(id)
    }

    /// The list `id`: the return value's attributes, then the parameters'.
    /// Empty when nothing has an attribute.
    #[inline]
    pub(crate) fn get(&self, id: u32) -> &[ParamAttrs] {
        let (start, len) = self.lists[id as usize];
        &self.pool[start as usize..start as usize + len as usize]
    }
}

// ---- function attributes ----

/// An unwind-table request: the `uwtable` attribute of a function, and the
/// `uwtable` module flag (§2.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
#[repr(u8)]
pub enum UwTable {
    /// No unwind table is required.
    #[default]
    None,
    /// `uwtable(sync)`: correct at call sites.
    Sync,
    /// `uwtable` or `uwtable(async)`: correct at every instruction.
    Async,
}

impl UwTable {
    /// The kind with LLVM's number (`UWTableKind`: 0, 1 or 2).
    pub const fn from_llvm(value: u64) -> Option<UwTable> {
        match value {
            0 => Some(UwTable::None),
            1 => Some(UwTable::Sync),
            2 => Some(UwTable::Async),
            _ => None,
        }
    }
}

/// The key of a kept string attribute of a function (§5.7).
///
/// The values are kept as the bytes LLVM has; each target reads the ones
/// it needs, once per distinct [`FnAttrsId`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum FnStr {
    /// `"target-cpu"`.
    TargetCpu,
    /// `"target-features"`.
    TargetFeatures,
    /// `"tune-cpu"`.
    TuneCpu,
    /// `"frame-pointer"`.
    FramePointer,
    /// `"patchable-function-entry"`.
    PatchableFunctionEntry,
    /// `"patchable-function-prefix"`.
    PatchableFunctionPrefix,
    /// `"function-instrument"`.
    FunctionInstrument,
    /// `"branch-target-enforcement"`.
    BranchTargetEnforcement,
    /// `"sign-return-address"`.
    SignReturnAddress,
    /// `"sign-return-address-key"`.
    SignReturnAddressKey,
    /// `"branch-protection-pauth-lr"`.
    BranchProtectionPauthLr,
    /// `"guarded-control-stack"`.
    GuardedControlStack,
    /// `"stack-probe-size"`.
    StackProbeSize,
    /// `"probe-stack"`.
    ProbeStack,
    /// `"split-stack"`.
    SplitStack,
    /// `"zero-call-used-regs"`.
    ZeroCallUsedRegs,
    /// `"stack-protector-buffer-size"`.
    StackProtectorBufferSize,
    /// `"min-legal-vector-width"`.
    MinLegalVectorWidth,
    /// `"no-jump-tables"`.
    NoJumpTables,
    /// `"no-builtins"`.
    NoBuiltins,
    /// `"no-builtin-<function>"`. The value of this key is the function's
    /// name, and a set may hold the key once per function.
    NoBuiltin,
}

impl FnStr {
    /// Every key, in order.
    pub const ALL: [FnStr; 21] = [
        FnStr::TargetCpu,
        FnStr::TargetFeatures,
        FnStr::TuneCpu,
        FnStr::FramePointer,
        FnStr::PatchableFunctionEntry,
        FnStr::PatchableFunctionPrefix,
        FnStr::FunctionInstrument,
        FnStr::BranchTargetEnforcement,
        FnStr::SignReturnAddress,
        FnStr::SignReturnAddressKey,
        FnStr::BranchProtectionPauthLr,
        FnStr::GuardedControlStack,
        FnStr::StackProbeSize,
        FnStr::ProbeStack,
        FnStr::SplitStack,
        FnStr::ZeroCallUsedRegs,
        FnStr::StackProtectorBufferSize,
        FnStr::MinLegalVectorWidth,
        FnStr::NoJumpTables,
        FnStr::NoBuiltins,
        FnStr::NoBuiltin,
    ];

    /// LLVM's attribute name; for [`FnStr::NoBuiltin`] the prefix
    /// `no-builtin-` that the function's name follows.
    pub const fn name(self) -> &'static str {
        match self {
            FnStr::TargetCpu => "target-cpu",
            FnStr::TargetFeatures => "target-features",
            FnStr::TuneCpu => "tune-cpu",
            FnStr::FramePointer => "frame-pointer",
            FnStr::PatchableFunctionEntry => "patchable-function-entry",
            FnStr::PatchableFunctionPrefix => "patchable-function-prefix",
            FnStr::FunctionInstrument => "function-instrument",
            FnStr::BranchTargetEnforcement => "branch-target-enforcement",
            FnStr::SignReturnAddress => "sign-return-address",
            FnStr::SignReturnAddressKey => "sign-return-address-key",
            FnStr::BranchProtectionPauthLr => "branch-protection-pauth-lr",
            FnStr::GuardedControlStack => "guarded-control-stack",
            FnStr::StackProbeSize => "stack-probe-size",
            FnStr::ProbeStack => "probe-stack",
            FnStr::SplitStack => "split-stack",
            FnStr::ZeroCallUsedRegs => "zero-call-used-regs",
            FnStr::StackProtectorBufferSize => "stack-protector-buffer-size",
            FnStr::MinLegalVectorWidth => "min-legal-vector-width",
            FnStr::NoJumpTables => "no-jump-tables",
            FnStr::NoBuiltins => "no-builtins",
            FnStr::NoBuiltin => "no-builtin-",
        }
    }

    /// The key of the string attribute `name`, with the function's name
    /// for a `no-builtin-<function>` attribute and nothing for the others.
    /// `None` if the attribute is not a kept one.
    pub fn from_name(name: &[u8]) -> Option<(FnStr, &[u8])> {
        for key in FnStr::ALL {
            if key == FnStr::NoBuiltin {
                match name.strip_prefix(key.name().as_bytes()) {
                    Some(function) if !function.is_empty() => return Some((key, function)),
                    _ => {}
                }
            } else if name == key.name().as_bytes() {
                return Some((key, &[]));
            }
        }
        None
    }
}

/// The kept attributes of one function (§5.7, "kept ... on functions"):
/// what a producer fills in and hands to `Module::intern_fn_attrs`.
///
/// ```
/// use hod_ir::{FnAttrs, FnStr, UwTable};
/// let mut attrs = FnAttrs::new();
/// attrs.set(FnAttrs::NOUNWIND | FnAttrs::SSP_STRONG)?;
/// attrs.set_uwtable(UwTable::Async);
/// attrs.set_string(FnStr::TargetCpu, b"generic")?;
/// assert!(attrs.has(FnAttrs::NOUNWIND));
/// assert_eq!(attrs.string(FnStr::TargetCpu), Some(&b"generic"[..]));
/// # Ok::<(), hod_ir::IrError>(())
/// ```
///
/// A value can be reused for the next function after [`FnAttrs::clear`];
/// it then allocates nothing.
#[derive(Clone, Debug, Default)]
pub struct FnAttrs {
    flags: u32,
    uwtable: UwTable,
    /// `alignstack(N)`: 0 for none, else log2(N) + 1.
    stack_align: u8,
    /// `(key, start, end)` in `bytes`, ordered by key and, for
    /// `NoBuiltin`, by value; one entry per key except `NoBuiltin`.
    strings: Vec<(FnStr, u32, u32)>,
    bytes: Vec<u8>,
}

impl FnAttrs {
    /// `noreturn`.
    pub const NORETURN: u32 = 1 << 0;
    /// `naked`.
    pub const NAKED: u32 = 1 << 1;
    /// `nounwind`.
    pub const NOUNWIND: u32 = 1 << 2;
    /// `returns_twice`.
    pub const RETURNS_TWICE: u32 = 1 << 3;
    /// `alwaysinline`.
    pub const ALWAYS_INLINE: u32 = 1 << 4;
    /// `ssp`.
    pub const SSP: u32 = 1 << 5;
    /// `sspreq`.
    pub const SSP_REQ: u32 = 1 << 6;
    /// `sspstrong`.
    pub const SSP_STRONG: u32 = 1 << 7;
    /// `presplitcoroutine`.
    pub const PRESPLIT_COROUTINE: u32 = 1 << 8;
    /// `sanitize_address`.
    pub const SANITIZE_ADDRESS: u32 = 1 << 9;
    /// `sanitize_alloc_token`.
    pub const SANITIZE_ALLOC_TOKEN: u32 = 1 << 10;
    /// `sanitize_hwaddress`.
    pub const SANITIZE_HWADDRESS: u32 = 1 << 11;
    /// `sanitize_memory`.
    pub const SANITIZE_MEMORY: u32 = 1 << 12;
    /// `sanitize_memtag`.
    pub const SANITIZE_MEMTAG: u32 = 1 << 13;
    /// `sanitize_numerical_stability`.
    pub const SANITIZE_NUMERICAL_STABILITY: u32 = 1 << 14;
    /// `sanitize_realtime`.
    pub const SANITIZE_REALTIME: u32 = 1 << 15;
    /// `sanitize_realtime_blocking`.
    pub const SANITIZE_REALTIME_BLOCKING: u32 = 1 << 16;
    /// `sanitize_thread`.
    pub const SANITIZE_THREAD: u32 = 1 << 17;
    /// `sanitize_type`.
    pub const SANITIZE_TYPE: u32 = 1 << 18;
    /// `convergent`: a constraint on transformations only (§5.7); hod
    /// keeps it so that the module it prints forbids what its input did.
    pub const CONVERGENT: u32 = 1 << 19;

    /// Every flag with LLVM's keyword for it, in bit order.
    pub const FLAG_NAMES: [(&'static str, u32); 20] = [
        ("noreturn", Self::NORETURN),
        ("naked", Self::NAKED),
        ("nounwind", Self::NOUNWIND),
        ("returns_twice", Self::RETURNS_TWICE),
        ("alwaysinline", Self::ALWAYS_INLINE),
        ("ssp", Self::SSP),
        ("sspreq", Self::SSP_REQ),
        ("sspstrong", Self::SSP_STRONG),
        ("presplitcoroutine", Self::PRESPLIT_COROUTINE),
        ("sanitize_address", Self::SANITIZE_ADDRESS),
        ("sanitize_alloc_token", Self::SANITIZE_ALLOC_TOKEN),
        ("sanitize_hwaddress", Self::SANITIZE_HWADDRESS),
        ("sanitize_memory", Self::SANITIZE_MEMORY),
        ("sanitize_memtag", Self::SANITIZE_MEMTAG),
        ("sanitize_numerical_stability", Self::SANITIZE_NUMERICAL_STABILITY),
        ("sanitize_realtime", Self::SANITIZE_REALTIME),
        ("sanitize_realtime_blocking", Self::SANITIZE_REALTIME_BLOCKING),
        ("sanitize_thread", Self::SANITIZE_THREAD),
        ("sanitize_type", Self::SANITIZE_TYPE),
        ("convergent", Self::CONVERGENT),
    ];

    const ALL_FLAGS: u32 = (1 << 20) - 1;

    /// No attributes.
    pub fn new() -> FnAttrs {
        FnAttrs::default()
    }

    /// Back to no attributes, keeping the storage.
    pub fn clear(&mut self) {
        self.flags = 0;
        self.uwtable = UwTable::None;
        self.stack_align = 0;
        self.strings.clear();
        self.bytes.clear();
    }

    /// The flag of the keyword attribute `name` (`b"nounwind"`), if it is
    /// one of the kept function flags.
    pub fn flag_named(name: &[u8]) -> Option<u32> {
        Self::FLAG_NAMES.iter().find(|(text, _)| text.as_bytes() == name).map(|&(_, flag)| flag)
    }

    /// Adds the flags in `flags` (an OR of the flag constants).
    ///
    /// # Errors
    /// `InvalidOperand` for a bit that is not a flag constant;
    /// `InvalidAttribute` for `sanitize_realtime` with
    /// `sanitize_realtime_blocking`, which LLVM calls incompatible (§5.7).
    pub fn set(&mut self, flags: u32) -> IrResult<()> {
        if flags & !Self::ALL_FLAGS != 0 {
            return err(
                IrErrorKind::InvalidOperand,
                format!("{flags:#x} is not a set of function flags"),
            );
        }
        let both = Self::SANITIZE_REALTIME | Self::SANITIZE_REALTIME_BLOCKING;
        if (self.flags | flags) & both == both {
            return err(
                IrErrorKind::InvalidAttribute,
                "sanitize_realtime and sanitize_realtime_blocking on one function",
            );
        }
        self.flags |= flags;
        Ok(())
    }

    /// Sets `uwtable`.
    pub fn set_uwtable(&mut self, kind: UwTable) {
        self.uwtable = kind;
    }

    /// Sets `alignstack(bytes)`.
    ///
    /// # Errors
    /// `InvalidOperand` unless `bytes` is a power of two up to 2^32.
    pub fn set_stack_align(&mut self, bytes: u64) -> IrResult<()> {
        self.stack_align = encode_align(bytes)?;
        Ok(())
    }

    /// Sets the string attribute `key` to `value`, replacing an earlier
    /// value. For [`FnStr::NoBuiltin`], `value` is a function's name and
    /// the name is added to the ones already present.
    ///
    /// # Errors
    /// `InvalidOperand` for [`FnStr::NoBuiltin`] with an empty name;
    /// `TableFull` past 4 GiB of values.
    pub fn set_string(&mut self, key: FnStr, value: &[u8]) -> IrResult<()> {
        let multi = key == FnStr::NoBuiltin;
        if multi && value.is_empty() {
            return err(IrErrorKind::InvalidOperand, "no-builtin- needs a function name");
        }
        // The position that keeps `strings` ordered by key, then (for
        // `NoBuiltin`) by value.
        let mut at = self.strings.len();
        for (i, &(k, start, end)) in self.strings.iter().enumerate() {
            let existing = &self.bytes[start as usize..end as usize];
            if k == key && (!multi || existing == value) {
                if existing == value {
                    return Ok(());
                }
                self.strings.remove(i);
                at = i;
                break;
            }
            if k > key || (k == key && existing > value) {
                at = i;
                break;
            }
        }
        let start = pool_start(self.bytes.len(), value.len(), "attribute value")?;
        self.bytes.extend_from_slice(value);
        self.strings.insert(at, (key, start, start + value.len() as u32));
        Ok(())
    }

    /// True if every flag in `flags` is set.
    #[inline]
    pub const fn has(&self, flags: u32) -> bool {
        self.flags & flags == flags
    }

    /// The flags, an OR of the flag constants.
    #[inline]
    pub const fn flags(&self) -> u32 {
        self.flags
    }

    /// `uwtable`.
    #[inline]
    pub const fn uwtable(&self) -> UwTable {
        self.uwtable
    }

    /// `alignstack(N)`, in bytes.
    #[inline]
    pub const fn stack_align(&self) -> Option<u64> {
        decode_align(self.stack_align)
    }

    /// The value of the string attribute `key`; for [`FnStr::NoBuiltin`]
    /// the first function name.
    pub fn string(&self, key: FnStr) -> Option<&[u8]> {
        self.strings().find(|&(k, _)| k == key).map(|(_, value)| value)
    }

    /// The string attributes, ordered by key.
    pub fn strings(&self) -> impl Iterator<Item = (FnStr, &[u8])> {
        self.strings.iter().map(|&(k, start, end)| (k, &self.bytes[start as usize..end as usize]))
    }

    /// True if no attribute is set.
    pub fn is_empty(&self) -> bool {
        self.flags == 0
            && self.uwtable == UwTable::None
            && self.stack_align == 0
            && self.strings.is_empty()
    }
}

/// An interned set of function attributes of a module (§5.7). Equal sets
/// have equal ids.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FnAttrsId(u32);

impl FnAttrsId {
    /// The set with no attributes, in every module.
    pub const EMPTY: FnAttrsId = FnAttrsId(0);

    /// The id with raw value `raw`. For the code that owns the table.
    #[inline]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// The raw value.
    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// The id as a table index.
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Debug for FnAttrsId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fnattrs{}", self.0)
    }
}

impl Default for FnAttrsId {
    fn default() -> Self {
        FnAttrsId::EMPTY
    }
}

/// A stored attribute set: 12 bytes.
#[derive(Clone, Copy, Debug)]
struct FnAttrsEntry {
    flags: u32,
    /// The set's first entry in `FnAttrTable::strings`.
    strings: u32,
    strings_len: u16,
    uwtable: UwTable,
    stack_align: u8,
}

/// The interned function attribute sets of a module.
#[derive(Clone, Debug)]
pub(crate) struct FnAttrTable<L: Layering = NoLayer> {
    sets: Seg<FnAttrsEntry, L>,
    strings: Seg<(FnStr, StrId), L>,
    values: StrPool<L>,
    interner: InternTable<L>,
}

impl<L: Layering> Default for FnAttrTable<L> {
    fn default() -> Self {
        let empty = FnAttrsEntry {
            flags: 0,
            strings: 0,
            strings_len: 0,
            uwtable: UwTable::None,
            stack_align: 0,
        };
        let mut sets = Seg::default();
        sets.push(empty);
        FnAttrTable {
            sets,
            strings: Seg::default(),
            values: StrPool::default(),
            interner: InternTable::new(),
        }
    }
}

impl FnAttrTable {
    pub(crate) fn seed(&self) -> FnAttrTable<Layered> {
        FnAttrTable {
            sets: self.sets.seed(),
            strings: self.strings.seed(),
            values: self.values.seed(),
            interner: self.interner.seed(),
        }
    }
}

impl FnAttrTable<Layered> {
    pub(crate) fn fork(&self) -> FnAttrTable<Layered> {
        FnAttrTable {
            sets: self.sets.fork(),
            strings: self.strings.fork(),
            values: self.values.fork(),
            interner: self.interner.fork(),
        }
    }

    pub(crate) fn local_is_empty(&self) -> bool {
        self.sets.local_is_empty()
    }

    /// The sets from `from` on, as owned values.
    pub(crate) fn owned_since(&self, from: usize) -> IrResult<Vec<FnAttrs>> {
        (from..self.sets.len())
            .map(|id| {
                let view = self.get(FnAttrsId(id as u32));
                let mut attrs = FnAttrs::new();
                attrs.flags = view.entry.flags;
                attrs.uwtable = view.entry.uwtable;
                attrs.stack_align = view.entry.stack_align;
                for (key, value) in view.strings() {
                    attrs.set_string(key, value)?;
                }
                Ok(attrs)
            })
            .collect()
    }
}

impl<L: Layering> FnAttrTable<L> {
    /// The id of the set equal to `attrs`, added if new.
    pub(crate) fn intern(&mut self, attrs: &FnAttrs) -> IrResult<FnAttrsId> {
        if attrs.is_empty() {
            return Ok(FnAttrsId::EMPTY);
        }
        let Ok(strings_len) = u16::try_from(attrs.strings.len()) else {
            return err(IrErrorKind::TableFull, "more than 65535 string attributes on a function");
        };
        let mut h = Hasher64::new(u64::from(attrs.flags));
        h.pair(attrs.uwtable as u32, u32::from(attrs.stack_align));
        for (key, value) in attrs.strings() {
            h.word(key as u64);
            h.bytes(value);
        }
        let hash = h.finish();
        let same = |id: u32| {
            let view = self.get(FnAttrsId(id));
            view.flags() == attrs.flags
                && view.uwtable() == attrs.uwtable
                && view.entry.stack_align == attrs.stack_align
                && view.entry.strings_len == strings_len
                && view.strings().eq(attrs.strings())
        };
        if let Some(found) = self.interner.find(hash, same) {
            return Ok(FnAttrsId(found));
        }
        let id = next_id(self.sets.len(), "function attribute")?;
        let strings = pool_start(self.strings.len(), attrs.strings.len(), "string attribute")?;
        let rollback = self.strings.len();
        for (key, value) in attrs.strings() {
            match self.values.intern(value) {
                Ok(value) => self.strings.push((key, value)),
                Err(error) => {
                    self.strings.truncate(rollback);
                    return Err(error);
                }
            }
        }
        self.sets.push(FnAttrsEntry {
            flags: attrs.flags,
            strings,
            strings_len,
            uwtable: attrs.uwtable,
            stack_align: attrs.stack_align,
        });
        self.interner.insert(hash, id);
        Ok(FnAttrsId(id))
    }

    /// The set `id`.
    #[inline]
    pub(crate) fn get(&self, id: FnAttrsId) -> FnAttrsView<'_, L> {
        FnAttrsView { table: self, entry: self.sets[id.index()] }
    }

    /// Number of distinct sets, the empty one included.
    pub(crate) fn len(&self) -> usize {
        self.sets.len()
    }
}

/// An interned set of function attributes, read through its module
/// (`Module::fn_attrs`).
#[derive(Clone, Copy)]
pub struct FnAttrsView<'a, L: Layering = NoLayer> {
    table: &'a FnAttrTable<L>,
    entry: FnAttrsEntry,
}

impl<'a, L: Layering> FnAttrsView<'a, L> {
    /// True if every flag in `flags` (an OR of the [`FnAttrs`] flag
    /// constants) is set.
    #[inline]
    pub const fn has(&self, flags: u32) -> bool {
        self.entry.flags & flags == flags
    }

    /// The flags, an OR of the [`FnAttrs`] flag constants.
    #[inline]
    pub const fn flags(&self) -> u32 {
        self.entry.flags
    }

    /// `uwtable`.
    #[inline]
    pub const fn uwtable(&self) -> UwTable {
        self.entry.uwtable
    }

    /// `alignstack(N)`, in bytes.
    #[inline]
    pub const fn stack_align(&self) -> Option<u64> {
        decode_align(self.entry.stack_align)
    }

    /// The value of the string attribute `key`; for [`FnStr::NoBuiltin`]
    /// the first function name.
    pub fn string(&self, key: FnStr) -> Option<&'a [u8]> {
        self.strings().find(|&(k, _)| k == key).map(|(_, value)| value)
    }

    /// The string attributes, ordered by key.
    pub fn strings(&self) -> impl Iterator<Item = (FnStr, &'a [u8])> + use<'a, L> {
        let table = self.table;
        let start = self.entry.strings as usize;
        table
            .strings
            .slice(start, usize::from(self.entry.strings_len))
            .iter()
            .map(move |&(key, value)| (key, table.values.get(value)))
    }

    /// True if the set has no attribute.
    pub fn is_empty(&self) -> bool {
        self.entry.flags == 0
            && self.entry.uwtable == UwTable::None
            && self.entry.stack_align == 0
            && self.entry.strings_len == 0
    }
}

impl<L: Layering> fmt::Debug for FnAttrsView<'_, L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut set = f.debug_set();
        for (name, flag) in FnAttrs::FLAG_NAMES {
            if self.has(flag) {
                set.entry(&format_args!("{name}"));
            }
        }
        if self.uwtable() != UwTable::None {
            set.entry(&format_args!("uwtable({:?})", self.uwtable()));
        }
        if let Some(bytes) = self.stack_align() {
            set.entry(&format_args!("alignstack({bytes})"));
        }
        for (key, value) in self.strings() {
            set.entry(&format_args!("{:?}={:?}", key.name(), String::from_utf8_lossy(value)));
        }
        set.finish()
    }
}

// ---- the attribute table ----

/// What hod does with an LLVM attribute (§5.7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AttrClass {
    /// It changes the generated code and has a representation.
    Kept,
    /// Only an optimizer reads it. Producers skip it.
    Dropped,
    /// Neither: a producer that meets it records `attr:<name>` as
    /// unsupported (`Module::record_unsupported`), and `classify` (§8)
    /// refuses the module naming it.
    Rejected,
}

/// How an attribute is written in LLVM IR. Keywords and strings are
/// separate name spaces: `nounwind` is not `"nounwind"`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AttrSyntax {
    /// A keyword, alone or with an argument: `noreturn`, `align 8`,
    /// `byval(%T)`, `memory(none)`, `range(i32 0, 10)`.
    Keyword,
    /// A string, alone or with a value: `"target-cpu"="generic"`.
    String,
}

/// One row of the attribute table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttrInfo {
    /// The attribute's name; for a prefix row, the prefix.
    pub name: &'static str,
    /// Keyword or string.
    pub syntax: AttrSyntax,
    /// The class.
    pub class: AttrClass,
    /// Whether the row stands for every name that starts with `name` and
    /// is longer (`"no-builtin-<function>"`).
    pub prefix: bool,
}

const fn kw(name: &'static str, class: AttrClass) -> AttrInfo {
    AttrInfo { name, syntax: AttrSyntax::Keyword, class, prefix: false }
}

const fn st(name: &'static str, class: AttrClass) -> AttrInfo {
    AttrInfo { name, syntax: AttrSyntax::String, class, prefix: false }
}

use AttrClass::{Dropped, Kept, Rejected};

/// The attribute table (§5.7): every keyword attribute of LLVM 19 to 22,
/// and every string attribute hod has a decision for, each in one class.
/// Keyword rows first, each group ordered by name. A name with no row is
/// [`AttrClass::Rejected`]; the `Rejected` rows are the names LLVM defines
/// that were looked at and deliberately left there.
pub const ATTRIBUTES: [AttrInfo; 140] = [
    kw("align", Kept),
    kw("alignstack", Kept),
    kw("allocalign", Dropped),
    kw("allockind", Dropped),
    kw("allocptr", Dropped),
    kw("allocsize", Dropped),
    kw("alwaysinline", Kept),
    kw("builtin", Dropped),
    kw("byref", Kept),
    kw("byval", Kept),
    kw("captures", Dropped),
    kw("cold", Dropped),
    kw("convergent", Kept),
    kw("coro_elide_safe", Rejected),
    kw("coro_only_destroy_when_complete", Rejected),
    kw("dead_on_return", Dropped),
    kw("dead_on_unwind", Dropped),
    kw("dereferenceable", Dropped),
    kw("dereferenceable_or_null", Dropped),
    kw("disable_sanitizer_instrumentation", Dropped),
    kw("elementtype", Kept),
    kw("fn_ret_thunk_extern", Rejected),
    kw("hot", Dropped),
    kw("hybrid_patchable", Rejected),
    kw("immarg", Dropped),
    kw("inalloca", Kept),
    kw("initializes", Dropped),
    kw("inlinehint", Dropped),
    kw("inreg", Kept),
    kw("jumptable", Rejected),
    kw("memory", Dropped),
    kw("minsize", Dropped),
    kw("mustprogress", Dropped),
    kw("naked", Kept),
    kw("nest", Kept),
    kw("noalias", Dropped),
    kw("nobuiltin", Dropped),
    kw("nocallback", Dropped),
    kw("nocapture", Dropped),
    kw("nocf_check", Rejected),
    kw("nocreateundeforpoison", Dropped),
    kw("nodivergencesource", Rejected),
    kw("noduplicate", Rejected),
    kw("noext", Dropped),
    kw("nofpclass", Dropped),
    kw("nofree", Dropped),
    kw("noimplicitfloat", Rejected),
    kw("noinline", Dropped),
    kw("nomerge", Dropped),
    kw("nonlazybind", Rejected),
    kw("nonnull", Dropped),
    kw("noprofile", Dropped),
    kw("norecurse", Dropped),
    kw("noredzone", Rejected),
    kw("noreturn", Kept),
    kw("nosanitize_bounds", Dropped),
    kw("nosanitize_coverage", Dropped),
    kw("nosync", Dropped),
    kw("noundef", Dropped),
    kw("nounwind", Kept),
    kw("null_pointer_is_valid", Dropped),
    kw("optdebug", Rejected),
    kw("optforfuzzing", Rejected),
    kw("optnone", Dropped),
    kw("optsize", Dropped),
    kw("preallocated", Kept),
    kw("presplitcoroutine", Kept),
    kw("range", Dropped),
    kw("readnone", Dropped),
    kw("readonly", Dropped),
    kw("returned", Dropped),
    kw("returns_twice", Kept),
    kw("safestack", Rejected),
    kw("sanitize_address", Kept),
    kw("sanitize_alloc_token", Kept),
    kw("sanitize_hwaddress", Kept),
    kw("sanitize_memory", Kept),
    kw("sanitize_memtag", Kept),
    kw("sanitize_numerical_stability", Kept),
    kw("sanitize_realtime", Kept),
    kw("sanitize_realtime_blocking", Kept),
    kw("sanitize_thread", Kept),
    kw("sanitize_type", Kept),
    kw("shadowcallstack", Rejected),
    kw("signext", Kept),
    kw("skipprofile", Dropped),
    kw("speculatable", Dropped),
    kw("speculative_load_hardening", Rejected),
    kw("sret", Kept),
    kw("ssp", Kept),
    kw("sspreq", Kept),
    kw("sspstrong", Kept),
    kw("strictfp", Rejected),
    kw("swiftasync", Kept),
    kw("swifterror", Kept),
    kw("swiftself", Kept),
    kw("uwtable", Kept),
    kw("vscale_range", Dropped),
    kw("willreturn", Dropped),
    kw("writable", Dropped),
    kw("writeonly", Dropped),
    kw("zeroext", Kept),
    st("allow_direct_access_in_hot_patch_function", Rejected),
    st("approx-func-fp-math", Dropped),
    st("branch-protection-pauth-lr", Kept),
    st("branch-target-enforcement", Kept),
    st("denormal-fp-math", Rejected),
    st("denormal-fp-math-f32", Rejected),
    st("fmv-features", Dropped),
    st("frame-pointer", Kept),
    st("function-instrument", Kept),
    st("guarded-control-stack", Kept),
    st("less-precise-fpmad", Dropped),
    st("loader-replaceable", Rejected),
    st("marked_for_windows_hot_patching", Rejected),
    st("min-legal-vector-width", Kept),
    AttrInfo { name: "no-builtin-", syntax: AttrSyntax::String, class: Kept, prefix: true },
    st("no-builtins", Kept),
    st("no-infs-fp-math", Dropped),
    st("no-inline-line-tables", Rejected),
    st("no-jump-tables", Kept),
    st("no-nans-fp-math", Dropped),
    st("no-signed-zeros-fp-math", Dropped),
    st("no-trapping-math", Dropped),
    st("patchable-function-entry", Kept),
    st("patchable-function-prefix", Kept),
    st("probe-stack", Kept),
    st("profile-sample-accurate", Rejected),
    st("sample-profile-suffix-elision-policy", Dropped),
    st("sign-return-address", Kept),
    st("sign-return-address-key", Kept),
    st("split-stack", Kept),
    st("stack-probe-size", Kept),
    st("stack-protector-buffer-size", Kept),
    st("target-cpu", Kept),
    st("target-features", Kept),
    st("tune-cpu", Kept),
    st("unsafe-fp-math", Dropped),
    st("use-sample-profile", Rejected),
    st("zero-call-used-regs", Kept),
];

/// Number of keyword rows at the start of [`ATTRIBUTES`].
const KEYWORD_ROWS: usize = 102;

/// The row of [`ATTRIBUTES`] for the attribute `name` written as `syntax`,
/// if it has one.
pub fn attr_info(syntax: AttrSyntax, name: &[u8]) -> Option<&'static AttrInfo> {
    let rows = match syntax {
        AttrSyntax::Keyword => &ATTRIBUTES[..KEYWORD_ROWS],
        AttrSyntax::String => &ATTRIBUTES[KEYWORD_ROWS..],
    };
    if let Ok(at) = rows.binary_search_by(|row| row.name.as_bytes().cmp(name))
        && !rows[at].prefix
    {
        return Some(&rows[at]);
    }
    // A miss looks only at the prefixed rows, not at every row.
    let prefixed = match syntax {
        AttrSyntax::Keyword => &[][..],
        AttrSyntax::String => &PREFIXED_STRING_ROWS[..],
    };
    prefixed
        .iter()
        .map(|&at| &ATTRIBUTES[at])
        .find(|row| name.len() > row.name.len() && name.starts_with(row.name.as_bytes()))
}

/// The positions in [`ATTRIBUTES`] of the rows that match by prefix (all
/// string rows), computed from the table (WP-101).
const PREFIXED_STRING_ROWS: [usize; prefixed_count()] = prefixed_rows();

/// The positions of the prefixed rows of [`ATTRIBUTES`], in order.
const fn prefixed_rows() -> [usize; prefixed_count()] {
    let mut rows = [0; prefixed_count()];
    let (mut at, mut n) = (0, 0);
    while at < ATTRIBUTES.len() {
        if ATTRIBUTES[at].prefix {
            rows[n] = at;
            n += 1;
        }
        at += 1;
    }
    rows
}

/// How many rows of [`ATTRIBUTES`] match by prefix.
const fn prefixed_count() -> usize {
    let (mut at, mut n) = (0, 0);
    while at < ATTRIBUTES.len() {
        if ATTRIBUTES[at].prefix {
            n += 1;
        }
        at += 1;
    }
    n
}

/// The class of the attribute `name` written as `syntax` (§5.7). A name
/// the table does not have is [`AttrClass::Rejected`].
pub fn attr_class(syntax: AttrSyntax, name: &[u8]) -> AttrClass {
    attr_info(syntax, name).map_or(Rejected, |row| row.class)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §5.7: calling conventions are LLVM's numbers; the keywords are the
    /// ones `llvm-dis` 22 prints for them.
    #[test]
    fn call_conv_numbers_and_keywords() {
        assert_eq!(size_of::<CallConv>(), 2);
        assert_eq!(CallConv::default(), CallConv::C);
        assert_eq!(CallConv::from_llvm(8).unwrap(), CallConv::FAST);
        assert_eq!(CallConv::FAST.llvm_id(), 8);
        let named = [
            (0, "ccc"),
            (8, "fastcc"),
            (9, "coldcc"),
            (10, "ghccc"),
            (13, "anyregcc"),
            (14, "preserve_mostcc"),
            (15, "preserve_allcc"),
            (16, "swiftcc"),
            (17, "cxx_fast_tlscc"),
            (18, "tailcc"),
            (19, "cfguard_checkcc"),
            (20, "swifttailcc"),
            (21, "preserve_nonecc"),
            (97, "aarch64_vector_pcs"),
            (98, "aarch64_sve_vector_pcs"),
        ];
        for (id, keyword) in named {
            assert_eq!(CallConv::from_llvm(id).unwrap().keyword(), Some(keyword), "cc {id}");
        }
        assert_eq!(CallConv::from_llvm(11).unwrap().keyword(), None);
        assert_eq!(CallConv::from_llvm(1023).unwrap().llvm_id(), 1023);
        assert_eq!(CallConv::from_llvm(1024).unwrap_err().kind, IrErrorKind::InvalidOperand);
        assert_eq!(CallConv::from_llvm(1 << 20).unwrap_err().kind, IrErrorKind::InvalidOperand);
    }

    /// §5.7: the kept parameter attributes fit eight bytes and read back.
    #[test]
    fn param_attrs_round_trip() {
        assert_eq!(size_of::<ParamAttrs>(), 8);
        assert!(ParamAttrs::NONE.is_none());
        assert_eq!(ParamAttrs::default(), ParamAttrs::NONE);
        assert_eq!(ParamAttrs::NONE.mem(), ParamMem::None);
        assert_eq!(ParamAttrs::NONE.mem_type(), None);
        assert_eq!(ParamAttrs::NONE.align(), None);
        assert_eq!(ParamAttrs::NONE.stack_align(), None);

        let kinds = [
            ParamMem::Sret,
            ParamMem::ByVal,
            ParamMem::ByRef,
            ParamMem::InAlloca,
            ParamMem::Preallocated,
            ParamMem::ElementType,
        ];
        for kind in kinds {
            let a = ParamAttrs::NONE.with_mem(kind, TypeId::I32).unwrap();
            assert_eq!(a.mem(), kind);
            assert_eq!(a.mem_type(), Some(TypeId::I32));
            assert!(!a.is_none());
        }

        let a = ParamAttrs::NONE
            .with(ParamAttrs::SIGNEXT | ParamAttrs::INREG)
            .unwrap()
            .with_mem(ParamMem::Sret, TypeId::I64)
            .unwrap()
            .with_align(1)
            .unwrap()
            .with_stack_align(1 << 32)
            .unwrap();
        assert!(a.has(ParamAttrs::SIGNEXT));
        assert!(a.has(ParamAttrs::SIGNEXT | ParamAttrs::INREG));
        assert!(!a.has(ParamAttrs::ZEROEXT));
        assert!(!a.has(ParamAttrs::SIGNEXT | ParamAttrs::NEST));
        assert_eq!(a.mem(), ParamMem::Sret);
        assert_eq!(a.align(), Some(1));
        assert_eq!(a.stack_align(), Some(1 << 32));

        let flags = [
            ParamAttrs::ZEROEXT,
            ParamAttrs::SIGNEXT,
            ParamAttrs::INREG,
            ParamAttrs::NEST,
            ParamAttrs::SWIFT_SELF,
            ParamAttrs::SWIFT_ERROR,
            ParamAttrs::SWIFT_ASYNC,
        ];
        for flag in flags {
            let a = ParamAttrs::NONE.with(flag).unwrap();
            assert!(a.has(flag));
            assert_eq!(
                a.mem(),
                ParamMem::None,
                "flag {flag:#x} must not read as a typed attribute"
            );
            for other in flags {
                assert_eq!(a.has(other), other == flag);
            }
        }
    }

    /// §5.7: what the representation cannot hold is an error, not a panic.
    #[test]
    fn param_attrs_errors() {
        let kind = |r: IrResult<ParamAttrs>| r.unwrap_err().kind;
        let both = ParamAttrs::ZEROEXT | ParamAttrs::SIGNEXT;
        assert_eq!(kind(ParamAttrs::NONE.with(both)), IrErrorKind::InvalidOperand);
        let zext = ParamAttrs::NONE.with(ParamAttrs::ZEROEXT).unwrap();
        assert_eq!(kind(zext.with(ParamAttrs::SIGNEXT)), IrErrorKind::InvalidOperand);
        assert_eq!(kind(ParamAttrs::NONE.with(1)), IrErrorKind::InvalidOperand);
        assert_eq!(kind(ParamAttrs::NONE.with(1 << 10)), IrErrorKind::InvalidOperand);
        assert_eq!(
            kind(ParamAttrs::NONE.with_mem(ParamMem::None, TypeId::I8)),
            IrErrorKind::InvalidOperand
        );
        let byval = ParamAttrs::NONE.with_mem(ParamMem::ByVal, TypeId::I8).unwrap();
        assert_eq!(
            kind(byval.with_mem(ParamMem::ElementType, TypeId::I8)),
            IrErrorKind::InvalidOperand
        );
        for bad in [0, 3, (1 << 32) + 1, 1 << 33] {
            assert_eq!(kind(ParamAttrs::NONE.with_align(bad)), IrErrorKind::InvalidOperand);
            assert_eq!(kind(ParamAttrs::NONE.with_stack_align(bad)), IrErrorKind::InvalidOperand);
        }
    }

    /// §5.7: a function's `[return, parameters..]` attribute lists are
    /// interned, and trailing `NONE`s are not part of a list.
    #[test]
    fn sig_attr_lists_are_interned() {
        let mut table = SigAttrTable::default();
        let sext = ParamAttrs::NONE.with(ParamAttrs::SIGNEXT).unwrap();
        let sret = ParamAttrs::NONE.with_mem(ParamMem::Sret, TypeId::I64).unwrap();
        assert_eq!(table.intern(ParamAttrs::NONE, &[]).unwrap(), SigAttrTable::EMPTY);
        assert_eq!(
            table.intern(ParamAttrs::NONE, &[ParamAttrs::NONE; 3]).unwrap(),
            SigAttrTable::EMPTY
        );
        assert!(table.get(SigAttrTable::EMPTY).is_empty());
        let a = table.intern(sext, &[]).unwrap();
        let b = table.intern(ParamAttrs::NONE, &[sret, ParamAttrs::NONE, sext]).unwrap();
        let c = table.intern(ParamAttrs::NONE, &[sret]).unwrap();
        assert_ne!(a, SigAttrTable::EMPTY);
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_eq!(table.intern(sext, &[ParamAttrs::NONE, ParamAttrs::NONE]).unwrap(), a);
        assert_eq!(table.intern(ParamAttrs::NONE, &[sret, ParamAttrs::NONE]).unwrap(), c);
        assert_eq!(table.get(a), &[sext]);
        assert_eq!(table.get(b), &[ParamAttrs::NONE, sret, ParamAttrs::NONE, sext]);
        assert_eq!(table.get(c), &[ParamAttrs::NONE, sret]);
    }

    /// §5.7: the kept function attributes read back from the builder, and
    /// string attributes are kept ordered with one value per key.
    #[test]
    fn fn_attrs_builder_round_trip() {
        let mut a = FnAttrs::new();
        assert!(a.is_empty());
        a.set(FnAttrs::NORETURN | FnAttrs::SANITIZE_TYPE).unwrap();
        a.set(FnAttrs::NOUNWIND).unwrap();
        a.set_uwtable(UwTable::Sync);
        a.set_stack_align(16).unwrap();
        a.set_string(FnStr::TuneCpu, b"generic").unwrap();
        a.set_string(FnStr::TargetCpu, b"x86-64").unwrap();
        a.set_string(FnStr::NoBuiltin, b"memcpy").unwrap();
        a.set_string(FnStr::NoBuiltin, b"bcmp").unwrap();
        a.set_string(FnStr::NoBuiltin, b"memcpy").unwrap();
        a.set_string(FnStr::TargetCpu, b"znver4").unwrap();
        a.set_string(FnStr::SplitStack, b"").unwrap();
        assert!(!a.is_empty());
        assert!(a.has(FnAttrs::NORETURN | FnAttrs::NOUNWIND | FnAttrs::SANITIZE_TYPE));
        assert!(!a.has(FnAttrs::NAKED));
        assert_eq!(a.flags(), FnAttrs::NORETURN | FnAttrs::NOUNWIND | FnAttrs::SANITIZE_TYPE);
        assert_eq!(a.uwtable(), UwTable::Sync);
        assert_eq!(a.stack_align(), Some(16));
        assert_eq!(a.string(FnStr::TargetCpu), Some(&b"znver4"[..]));
        assert_eq!(a.string(FnStr::SplitStack), Some(&b""[..]));
        assert_eq!(a.string(FnStr::ProbeStack), None);
        let strings: Vec<(FnStr, &[u8])> = a.strings().collect();
        assert_eq!(
            strings,
            [
                (FnStr::TargetCpu, &b"znver4"[..]),
                (FnStr::TuneCpu, b"generic"),
                (FnStr::SplitStack, b""),
                (FnStr::NoBuiltin, b"bcmp"),
                (FnStr::NoBuiltin, b"memcpy"),
            ]
        );
        a.clear();
        assert!(a.is_empty());
        assert_eq!(a.strings().count(), 0);

        let kind = |r: IrResult<()>| r.unwrap_err().kind;
        assert_eq!(kind(a.set(1 << 20)), IrErrorKind::InvalidOperand);
        assert_eq!(kind(a.set_stack_align(24)), IrErrorKind::InvalidOperand);
        assert_eq!(kind(a.set_string(FnStr::NoBuiltin, b"")), IrErrorKind::InvalidOperand);
        assert!(a.is_empty());

        assert_eq!(UwTable::from_llvm(0), Some(UwTable::None));
        assert_eq!(UwTable::from_llvm(1), Some(UwTable::Sync));
        assert_eq!(UwTable::from_llvm(2), Some(UwTable::Async));
        assert_eq!(UwTable::from_llvm(3), None);
    }

    /// §5.7: equal attribute sets are one id whatever order they were
    /// built in; a stored set is 12 bytes.
    #[test]
    fn fn_attrs_are_interned() {
        assert_eq!(size_of::<FnAttrsEntry>(), 12);
        assert_eq!(size_of::<FnAttrsId>(), 4);
        assert_eq!(size_of::<(FnStr, StrId)>(), 8);
        let mut table = <FnAttrTable>::default();
        assert_eq!(table.intern(&FnAttrs::new()).unwrap(), FnAttrsId::EMPTY);
        assert!(table.get(FnAttrsId::EMPTY).is_empty());
        assert_eq!(FnAttrsId::default(), FnAttrsId::EMPTY);

        let mut a = FnAttrs::new();
        a.set(FnAttrs::NOUNWIND).unwrap();
        a.set_string(FnStr::TargetCpu, b"generic").unwrap();
        a.set_string(FnStr::FramePointer, b"all").unwrap();
        let mut b = FnAttrs::new();
        b.set_string(FnStr::FramePointer, b"none").unwrap();
        b.set_string(FnStr::FramePointer, b"all").unwrap();
        b.set_string(FnStr::TargetCpu, b"generic").unwrap();
        b.set(FnAttrs::NOUNWIND).unwrap();
        let id = table.intern(&a).unwrap();
        assert_ne!(id, FnAttrsId::EMPTY);
        assert_eq!(table.intern(&b).unwrap(), id);
        assert_eq!(table.len(), 2);

        // Each field takes part in identity.
        let mut variants = Vec::new();
        let mut c = a.clone();
        c.set(FnAttrs::NAKED).unwrap();
        variants.push(c);
        let mut c = a.clone();
        c.set_uwtable(UwTable::Async);
        variants.push(c);
        let mut c = a.clone();
        c.set_stack_align(8).unwrap();
        variants.push(c);
        let mut c = a.clone();
        c.set_string(FnStr::TargetCpu, b"generic2").unwrap();
        variants.push(c);
        let mut c = a.clone();
        c.set_string(FnStr::TuneCpu, b"generic").unwrap();
        variants.push(c);
        let mut c = FnAttrs::new();
        c.set(FnAttrs::NOUNWIND).unwrap();
        variants.push(c);
        let mut ids = vec![FnAttrsId::EMPTY, id];
        for variant in &variants {
            let new = table.intern(variant).unwrap();
            assert!(!ids.contains(&new), "{variant:?} is a set of its own");
            assert_eq!(table.intern(variant).unwrap(), new);
            ids.push(new);
        }

        let view = table.get(id);
        assert!(view.has(FnAttrs::NOUNWIND));
        assert_eq!(view.flags(), FnAttrs::NOUNWIND);
        assert_eq!(view.uwtable(), UwTable::None);
        assert_eq!(view.stack_align(), None);
        assert_eq!(view.string(FnStr::TargetCpu), Some(&b"generic"[..]));
        assert_eq!(view.string(FnStr::TuneCpu), None);
        assert_eq!(view.strings().count(), 2);
        assert!(!view.is_empty());
        assert_eq!(
            format!("{view:?}"),
            r#"{nounwind, "target-cpu"="generic", "frame-pointer"="all"}"#
        );
        assert_eq!(format!("{id:?}"), "fnattrs1");
        assert_eq!(table.get(ids[4]).stack_align(), Some(8));
        assert_eq!(table.get(ids[3]).uwtable(), UwTable::Async);
    }

    /// §5.7: the table is ordered (the lookup is a binary search), has no
    /// name twice, and agrees with the representation: every kept string
    /// row is an `FnStr` key and every function flag a kept keyword.
    #[test]
    fn attribute_table_is_ordered_and_matches_the_representation() {
        let (keywords, strings) = ATTRIBUTES.split_at(KEYWORD_ROWS);
        assert!(keywords.iter().all(|row| row.syntax == AttrSyntax::Keyword && !row.prefix));
        assert!(strings.iter().all(|row| row.syntax == AttrSyntax::String));
        let prefixed: Vec<usize> =
            (0..ATTRIBUTES.len()).filter(|&at| ATTRIBUTES[at].prefix).collect();
        assert_eq!(prefixed, PREFIXED_STRING_ROWS, "the prefixed rows a miss looks at");
        for rows in [keywords, strings] {
            for pair in rows.windows(2) {
                assert!(pair[0].name < pair[1].name, "{} before {}", pair[0].name, pair[1].name);
            }
        }
        for row in &ATTRIBUTES {
            let name = if row.prefix { format!("{}x", row.name) } else { row.name.to_string() };
            assert_eq!(attr_info(row.syntax, name.as_bytes()), Some(row));
            assert_eq!(attr_class(row.syntax, name.as_bytes()), row.class);
        }
        for row in strings {
            let key = FnStr::from_name(if row.prefix {
                b"no-builtin-memcpy"
            } else {
                row.name.as_bytes()
            });
            assert_eq!(key.is_some(), row.class == Kept, "{}", row.name);
        }
        for key in FnStr::ALL {
            let name = if key == FnStr::NoBuiltin { "no-builtin-memset" } else { key.name() };
            assert_eq!(attr_class(AttrSyntax::String, name.as_bytes()), Kept, "{name}");
            let function: &[u8] = if key == FnStr::NoBuiltin { b"memset" } else { b"" };
            assert_eq!(FnStr::from_name(name.as_bytes()), Some((key, function)));
        }
        for (name, flag) in FnAttrs::FLAG_NAMES {
            assert_eq!(attr_class(AttrSyntax::Keyword, name.as_bytes()), Kept, "{name}");
            assert_eq!(FnAttrs::flag_named(name.as_bytes()), Some(flag));
        }
        assert_eq!(FnAttrs::flag_named(b"noinline"), None);
        assert_eq!(FnStr::from_name(b"no-builtin-"), None);
        assert_eq!(FnStr::from_name(b"no-trapping-math"), None);
    }

    /// §5.7: a name in neither list is rejected, and the two name spaces
    /// are separate.
    #[test]
    fn unknown_attributes_are_rejected() {
        assert_eq!(attr_class(AttrSyntax::Keyword, b"noundef"), Dropped);
        assert_eq!(attr_class(AttrSyntax::Keyword, b"sret"), Kept);
        assert_eq!(attr_class(AttrSyntax::Keyword, b"jumptable"), Rejected);
        assert!(attr_info(AttrSyntax::Keyword, b"jumptable").is_some());
        assert_eq!(attr_class(AttrSyntax::Keyword, b"some_future_attribute"), Rejected);
        assert_eq!(attr_info(AttrSyntax::Keyword, b"some_future_attribute"), None);
        assert_eq!(attr_class(AttrSyntax::String, b"some-future-attribute"), Rejected);
        assert_eq!(attr_class(AttrSyntax::String, b"noundef"), Rejected);
        assert_eq!(attr_class(AttrSyntax::Keyword, b"target-cpu"), Rejected);
        assert_eq!(attr_class(AttrSyntax::String, b"target-cpu"), Kept);
        assert_eq!(attr_class(AttrSyntax::String, b"no-builtin-memcpy"), Kept);
        assert_eq!(attr_class(AttrSyntax::String, b"no-builtin-"), Rejected);
        assert_eq!(attr_class(AttrSyntax::String, b"no-builtins"), Kept);
        assert_eq!(attr_class(AttrSyntax::String, b"no-builtin"), Rejected);
        assert_eq!(attr_class(AttrSyntax::String, b""), Rejected);
    }
}
