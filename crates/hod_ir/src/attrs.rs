//! Calling conventions and parameter attributes (`spec/ir.md` §5.7): the
//! two attribute types that function symbols (§2.2) and call instructions
//! (§5.4) share.
//!
//! Only attributes that change the generated code are representable. The
//! table that classifies every LLVM attribute name as kept, dropped or
//! rejected, and the function-level attributes, are added to this module by
//! the work package that owns symbols.

use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::TypeId;

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
}

impl Default for ParamAttrs {
    fn default() -> Self {
        ParamAttrs::NONE
    }
}

fn encode_align(bytes: u64) -> IrResult<u8> {
    if !bytes.is_power_of_two() || bytes > 1 << 32 {
        return err(
            IrErrorKind::InvalidOperand,
            format!("alignment {bytes} is not a power of two up to 2^32"),
        );
    }
    // A power of two up to 2^32 has at most 32 trailing zeros.
    Ok(u8::try_from(bytes.trailing_zeros()).unwrap_or(32) + 1)
}

const fn decode_align(code: u8) -> Option<u64> {
    if code == 0 { None } else { Some(1 << (code - 1)) }
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
}
