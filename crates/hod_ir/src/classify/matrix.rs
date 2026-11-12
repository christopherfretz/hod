//! The support matrix (`spec/ir.md` §8.2 to §8.4): one row per construct
//! `hod_ir` can represent, with its status on each target.
//!
//! **This file is data.** Implementing a construct is a change to its row
//! here (and a test): `a64("inst:add", "WP-035", "")` becomes
//! `row("inst:add", on(&[TypeClass::Int]), todo(X64), "")`. Nothing else
//! in `classify` changes.
//!
//! Several groups are indexed by an enum of the IR (`Opcode`, `RmwOp`,
//! `Linkage`, ...): their rows are in the enum's order and their arrays
//! have the enum's length, so adding a member without a row does not
//! compile, and `tests/classify_matrix.rs` checks the names one by one.

use std::fmt;

use crate::attrs::{FnAttrs, FnStr};
use crate::comdat::ComdatKind;
use crate::constant::ConstOp;
use crate::ids::TypeId;
use crate::inst::{Opcode, RmwOp};
use crate::intrinsic::Intrinsic;
use crate::symbol::Linkage;
use crate::target::Arch;
use crate::types::{TypeKind, TypeTable};

// ---- type classes ----

/// The class of a first-class type, as far as support differs (§8.3).
///
/// Every value an instruction produces or consumes has one. A matrix row
/// that operates on values is supported *for a set of classes*.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum TypeClass {
    /// An integer of at most 64 bits.
    Int,
    /// `i128`.
    I128,
    /// Any other integer wider than 64 bits.
    IntWide,
    /// A pointer in address space 0.
    Ptr,
    /// `half`.
    Half,
    /// `bfloat`.
    BFloat,
    /// `float`.
    Float,
    /// `double`.
    Double,
    /// `fp128`.
    Fp128,
    /// `x86_fp80`.
    X86Fp80,
    /// A vector of fixed length.
    Vector,
    /// A scalable vector.
    ScalableVector,
    /// A struct or an array as a first-class value.
    Aggregate,
    /// `token`.
    Token,
    /// A target extension type.
    TargetExt,
}

impl TypeClass {
    /// Every class, in order.
    pub const ALL: [TypeClass; 15] = [
        TypeClass::Int,
        TypeClass::I128,
        TypeClass::IntWide,
        TypeClass::Ptr,
        TypeClass::Half,
        TypeClass::BFloat,
        TypeClass::Float,
        TypeClass::Double,
        TypeClass::Fp128,
        TypeClass::X86Fp80,
        TypeClass::Vector,
        TypeClass::ScalableVector,
        TypeClass::Aggregate,
        TypeClass::Token,
        TypeClass::TargetExt,
    ];

    /// The class's name: what follows `type:` in its row's name and the
    /// `/` of a pair name (§8.5).
    pub const fn name(self) -> &'static str {
        match self {
            TypeClass::Int => "int",
            TypeClass::I128 => "i128",
            TypeClass::IntWide => "int-wide",
            TypeClass::Ptr => "ptr",
            TypeClass::Half => "half",
            TypeClass::BFloat => "bfloat",
            TypeClass::Float => "float",
            TypeClass::Double => "double",
            TypeClass::Fp128 => "fp128",
            TypeClass::X86Fp80 => "x86_fp80",
            TypeClass::Vector => "vector",
            TypeClass::ScalableVector => "scalable-vector",
            TypeClass::Aggregate => "aggregate",
            TypeClass::Token => "token",
            TypeClass::TargetExt => "target-ext",
        }
    }

    /// The class called `name`.
    pub fn named(name: &str) -> Option<TypeClass> {
        TypeClass::ALL.into_iter().find(|class| class.name() == name)
    }

    /// The class of type `ty`: `None` for a type that has no values
    /// (`void`, `label`, `metadata`, a function type, an opaque struct)
    /// and for an id that is not a type of `types`. A pointer is `Ptr`
    /// whatever its address space; `classify` refuses the non-zero ones
    /// by a rule of their own (§8.6).
    pub fn of(types: &TypeTable, ty: TypeId) -> Option<TypeClass> {
        match class_code(types, ty) {
            FAR_PTR => Some(TypeClass::Ptr),
            code => TypeClass::ALL.get(usize::from(code)).copied(),
        }
    }

    #[inline]
    pub(crate) const fn bit(self) -> u16 {
        1 << self as u8
    }
}

/// [`class_code`] of a pointer in a non-zero address space.
pub(crate) const FAR_PTR: u8 = 0xfe;
/// [`class_code`] of a type without values, or of no type at all.
pub(crate) const NO_VALUE: u8 = 0xff;

/// The class codes of the types with fixed ids (`TypeId::VOID` to
/// `TypeId::I128`).
const FIXED_CLASSES: [u8; TypeId::FIXED_COUNT as usize] = [
    NO_VALUE,                 // void
    NO_VALUE,                 // label
    NO_VALUE,                 // metadata
    TypeClass::Token as u8,   // token
    TypeClass::Half as u8,    // half
    TypeClass::BFloat as u8,  // bfloat
    TypeClass::Float as u8,   // float
    TypeClass::Double as u8,  // double
    TypeClass::Fp128 as u8,   // fp128
    TypeClass::X86Fp80 as u8, // x86_fp80
    TypeClass::Ptr as u8,     // ptr
    TypeClass::Int as u8,     // i1
    TypeClass::Int as u8,     // i8
    TypeClass::Int as u8,     // i16
    TypeClass::Int as u8,     // i32
    TypeClass::Int as u8,     // i64
    TypeClass::I128 as u8,    // i128
];

/// The class of `ty` as a number: a [`TypeClass`] as `u8`, [`FAR_PTR`] or
/// [`NO_VALUE`]. Never panics, whatever `ty` is.
#[inline]
pub(crate) fn class_code(types: &TypeTable, ty: TypeId) -> u8 {
    match FIXED_CLASSES.get(ty.index()) {
        Some(&code) => code,
        None => class_code_slow(types, ty),
    }
}

fn class_code_slow(types: &TypeTable, ty: TypeId) -> u8 {
    if ty.index() >= types.len() {
        return NO_VALUE;
    }
    // Exhaustive on purpose: a new `TypeKind` has to be given a class.
    let class = match types.kind(ty) {
        TypeKind::Void
        | TypeKind::Label
        | TypeKind::Metadata
        | TypeKind::Opaque(_)
        | TypeKind::Function { .. } => return NO_VALUE,
        TypeKind::Token => TypeClass::Token,
        TypeKind::Half => TypeClass::Half,
        TypeKind::BFloat => TypeClass::BFloat,
        TypeKind::Float => TypeClass::Float,
        TypeKind::Double => TypeClass::Double,
        TypeKind::Fp128 => TypeClass::Fp128,
        TypeKind::X86Fp80 => TypeClass::X86Fp80,
        TypeKind::Int(bits) if bits <= 64 => TypeClass::Int,
        TypeKind::Int(128) => TypeClass::I128,
        TypeKind::Int(_) => TypeClass::IntWide,
        TypeKind::Ptr { addr_space: 0 } => TypeClass::Ptr,
        TypeKind::Ptr { .. } => return FAR_PTR,
        TypeKind::Vector { .. } => TypeClass::Vector,
        TypeKind::ScalableVector { .. } => TypeClass::ScalableVector,
        TypeKind::Array { .. } | TypeKind::Struct { .. } => TypeClass::Aggregate,
        TypeKind::TargetExt { .. } => TypeClass::TargetExt,
    };
    class as u8
}

/// A set of [`TypeClass`]es.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TypeClasses(u16);

impl TypeClasses {
    /// No class.
    pub const NONE: TypeClasses = TypeClasses(0);
    /// Every class.
    pub const ALL: TypeClasses = TypeClasses((1 << TypeClass::ALL.len()) - 1);

    /// The set of `classes`.
    pub const fn of(classes: &[TypeClass]) -> TypeClasses {
        let mut bits = 0;
        let mut index = 0;
        while index < classes.len() {
            bits |= classes[index].bit();
            index += 1;
        }
        TypeClasses(bits)
    }

    /// Whether `class` is a member.
    #[inline]
    pub const fn has(self, class: TypeClass) -> bool {
        self.0 & class.bit() != 0
    }

    /// Whether the set has no member.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The members, in [`TypeClass::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = TypeClass> {
        TypeClass::ALL.into_iter().filter(move |&class| self.has(class))
    }

    #[inline]
    pub(crate) const fn bits(self) -> u16 {
        self.0
    }

    #[inline]
    pub(crate) const fn from_bits(bits: u16) -> TypeClasses {
        TypeClasses(bits & Self::ALL.0)
    }
}

impl fmt::Debug for TypeClasses {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter().map(TypeClass::name)).finish()
    }
}

// ---- rows ----

/// What hod does with a construct on one target (§8.2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SupportStatus {
    /// Code generation implements it, and a test says so.
    Supported,
    /// It will be implemented; the text names the work package, milestone
    /// or track that flips the row (`docs/BACKLOG.md`), or `unplanned`.
    NotImplemented(&'static str),
    /// It is refused by design; the text says why. No [`Support`] accepts
    /// it.
    ///
    /// [`Support`]: crate::Support
    Rejected(&'static str),
}

/// One target's entry of a [`SupportRow`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SupportCell {
    /// The status.
    pub status: SupportStatus,
    /// For a supported row that operates on values (§8.3): the classes it
    /// is implemented for. [`TypeClasses::ALL`] for a supported row of
    /// any other kind, empty for a row that is not supported.
    pub classes: TypeClasses,
}

impl SupportCell {
    /// Supported, for every class.
    pub const SUPPORTED: SupportCell =
        SupportCell { status: SupportStatus::Supported, classes: TypeClasses::ALL };

    /// Supported for the values of `classes` only.
    pub const fn supported_for(classes: &[TypeClass]) -> SupportCell {
        SupportCell { status: SupportStatus::Supported, classes: TypeClasses::of(classes) }
    }

    /// Not implemented yet; `by` names what will implement it.
    pub const fn not_implemented(by: &'static str) -> SupportCell {
        SupportCell { status: SupportStatus::NotImplemented(by), classes: TypeClasses::NONE }
    }

    /// Refused by design, because of `why`.
    pub const fn rejected(why: &'static str) -> SupportCell {
        SupportCell { status: SupportStatus::Rejected(why), classes: TypeClasses::NONE }
    }
}

/// One construct of the support matrix (§8.2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SupportRow {
    /// The construct's stable name, `group:name` (§8.5).
    pub name: &'static str,
    /// Its status for `aarch64-unknown-linux-gnu`.
    pub aarch64: SupportCell,
    /// Its status for `x86_64-unknown-linux-gnu`.
    pub x86_64: SupportCell,
    /// What the name covers, where the name does not say it.
    pub note: &'static str,
}

impl SupportRow {
    /// The group: the name up to its colon.
    pub fn group(&self) -> &'static str {
        self.name.split_once(':').map_or(self.name, |(group, _)| group)
    }

    /// The entry for `arch`.
    pub const fn cell(&self, arch: Arch) -> SupportCell {
        match arch {
            Arch::Aarch64 => self.aarch64,
            Arch::X86_64 => self.x86_64,
        }
    }
}

/// A construct name that is not a matrix row: a structural rule, a
/// malformed-input outcome, or a family of names built from the input.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ConstructInfo {
    /// The name; for a family, with the variable part in angle brackets.
    pub name: &'static str,
    /// When it is reported.
    pub what: &'static str,
}

// ---- how a row is written ----

/// The label of everything that waits for the x86-64 target.
const X64: &str = "T-X64";
/// No backlog row covers it yet.
const LATER: &str = "unplanned";
const VECTORS: &str = "M2 (vectors)";
const CORO: &str = "T-XF (coroutine splitting)";
const DEBUG: &str = "T-DW";
const ASM: &str = "hod_asm";

const WINDOWS_EH: &str = "Windows funclet exception handling; not an ELF construct";
const WINDOWS: &str = "a Windows construct";
const SCALABLE: &str = "scalable vectors are refused by design";
const A64_ONLY: &str = "an AArch64 property";
const X64_ONLY: &str = "an x86 property";
const NO_FORM: &str = "a constant expression with no normal form (§4.2)";
const ELF_COMDAT: &str = "ELF has `any` and `nodeduplicate` only; `llc` refuses it too";
const GC: &str = "garbage-collection statepoints are refused by design";

const ON: SupportCell = SupportCell::SUPPORTED;

/// Supported for `classes`: what a work package writes when it flips a
/// row that operates on values.
const fn on(classes: &[TypeClass]) -> SupportCell {
    SupportCell::supported_for(classes)
}

const fn todo(by: &'static str) -> SupportCell {
    SupportCell::not_implemented(by)
}

const fn never(why: &'static str) -> SupportCell {
    SupportCell::rejected(why)
}

const fn row(
    name: &'static str,
    aarch64: SupportCell,
    x86_64: SupportCell,
    note: &'static str,
) -> SupportRow {
    SupportRow { name, aarch64, x86_64, note }
}

/// The same on both targets.
const fn both(name: &'static str, cell: SupportCell, note: &'static str) -> SupportRow {
    row(name, cell, cell, note)
}

/// Not implemented: on AArch64 until `by`, on x86-64 until that target is
/// written.
const fn a64(name: &'static str, by: &'static str, note: &'static str) -> SupportRow {
    row(name, todo(by), todo(X64), note)
}

// ---- the groups, in the order of `docs/status/support.md` ----

const MODULE: [SupportRow; 24] = [
    row("module:pic", ON, todo(X64), "`PIC Level` is not 0"),
    row("module:pie", ON, todo(X64), "`PIE Level` is not 0"),
    // `CodeModel` order.
    a64("module:code-model-tiny", LATER, ""),
    both("module:code-model-small", ON, "the default of both targets: nothing changes"),
    a64("module:code-model-kernel", LATER, ""),
    a64("module:code-model-medium", LATER, ""),
    a64("module:code-model-large", LATER, ""),
    a64("module:large-data-threshold", LATER, ""),
    row(
        "module:uwtable",
        ON,
        todo(X64),
        "the module's default unwind-table kind is not `none` (WP-049)",
    ),
    both(
        "module:frame-pointer",
        ON,
        "any `frame-pointer` value: hod always keeps a frame pointer (`spec/codegen.md` §5)",
    ),
    // `BranchProtection` bit order.
    row("module:branch-target-enforcement", todo(LATER), never(A64_ONLY), ""),
    row("module:guarded-control-stack", todo(LATER), never(A64_ONLY), ""),
    row("module:sign-return-address", todo(LATER), never(A64_ONLY), ""),
    row("module:sign-return-address-all", todo(LATER), never(A64_ONLY), ""),
    row("module:sign-return-address-with-bkey", todo(LATER), never(A64_ONLY), ""),
    row("module:branch-protection-pauth-lr", todo(LATER), never(A64_ONLY), ""),
    row("module:cf-protection-branch", never(X64_ONLY), todo(X64), ""),
    row("module:cf-protection-return", never(X64_ONLY), todo(X64), ""),
    a64("module:asm", ASM, "module-level inline assembly an assembler must read"),
    row(
        "module:asm-symbols",
        ON,
        todo(X64),
        "module-level assembly made only of symbol directives (`spec/ir.md` §2.3)",
    ),
    row("module:linker-options", ON, todo(X64), "`!llvm.linker.options`"),
    row("module:dependent-libraries", ON, todo(X64), "`!llvm.dependent-libraries`"),
    row("module:global-ctors", ON, todo(X64), "an entry of `llvm.global_ctors`"),
    row("module:global-dtors", ON, todo(X64), "an entry of `llvm.global_dtors`"),
];

const STRUCTOR: [SupportRow; 2] = [
    row(
        "structor:priority",
        ON,
        todo(X64),
        "a constructor or destructor priority other than 65535",
    ),
    row(
        "structor:associated",
        ON,
        todo(X64),
        "a constructor or destructor with an associated symbol",
    ),
];

const DEBUG_ROWS: [SupportRow; 4] = [
    a64("debug:module-flags", DEBUG, "`Dwarf Version` or `Debug Info Version` is set"),
    a64("debug:subprogram", DEBUG, "a function with a body and a `DISubprogram`"),
    a64("debug:slot-variables", DEBUG, "a stack slot that is a source variable"),
    a64("debug:locations", DEBUG, "a body whose instructions carry source locations"),
];

const GLOBAL: [SupportRow; 12] = [
    both("global:function-declaration", ON, "nothing is emitted for a declaration"),
    both("global:variable-declaration", ON, "nothing is emitted for a declaration"),
    row("global:function-definition", ON, todo(X64), "a function that gets a body"),
    row("global:variable-definition", ON, todo(X64), "a variable with an initializer"),
    row("global:alias", ON, todo(X64), ""),
    row("global:alias-offset", ON, todo(X64), "an alias of a symbol plus a non-zero offset"),
    a64("global:alias-absolute", LATER, "an alias of a number"),
    row("global:ifunc", ON, todo(X64), ""),
    row("global:section", ON, todo(X64), "a definition with an explicit section"),
    row("global:align", ON, todo(X64), "a definition with a stated alignment"),
    row("global:unnamed-addr", ON, todo(X64), "`unnamed_addr` on a definition: mergeable sections"),
    row("global:used", ON, todo(X64), "a definition in `llvm.used` (`SHF_GNU_RETAIN`)"),
];

const LINKAGE: [SupportRow; Linkage::ALL.len()] = [
    both("linkage:external", ON, "by itself; a definition also meets its `global:` row"),
    a64("linkage:available_externally", "WP-021", "design 0001, point 6"),
    row("linkage:linkonce", ON, todo(X64), ""),
    row("linkage:linkonce_odr", ON, todo(X64), ""),
    row("linkage:weak", ON, todo(X64), ""),
    row("linkage:weak_odr", ON, todo(X64), ""),
    both(
        "linkage:appending",
        never("only LLVM's special arrays have it, and those are not symbols (§2.2)"),
        "",
    ),
    row("linkage:internal", ON, todo(X64), ""),
    row("linkage:private", ON, todo(X64), ""),
    row("linkage:extern_weak", ON, todo(X64), ""),
    row("linkage:common", ON, todo(X64), ""),
];

const VISIBILITY: [SupportRow; 2] =
    [row("visibility:hidden", ON, todo(X64), ""), row("visibility:protected", ON, todo(X64), "")];

const TLS_ROWS: [SupportRow; 4] = [
    row("tls:general-dynamic", ON, todo(X64), "`thread_local`"),
    row("tls:local-dynamic", ON, todo(X64), "`thread_local(localdynamic)`"),
    row("tls:initial-exec", ON, todo(X64), "`thread_local(initialexec)`"),
    row("tls:local-exec", ON, todo(X64), "`thread_local(localexec)`"),
];

const COMDAT: [SupportRow; ComdatKind::ALL.len()] = [
    row("comdat:any", ON, todo(X64), ""),
    both("comdat:exactmatch", never(ELF_COMDAT), ""),
    both("comdat:largest", never(ELF_COMDAT), ""),
    row("comdat:nodeduplicate", ON, todo(X64), ""),
    both("comdat:samesize", never(ELF_COMDAT), ""),
];

const INIT: [SupportRow; 8] = [
    row("init:data", ON, todo(X64), "any initializer: numbers, zero, `undef`, aggregates, bytes"),
    row("init:symbol", ON, todo(X64), "a symbol's address, with or without an offset"),
    row("init:blockaddress", ON, todo(X64), "of a block of a function with an `indirectbr` (§9.6)"),
    row("init:sym-diff", ON, todo(X64), "the difference of two symbol addresses"),
    a64("init:sym-diff-plt", LATER, "the same through `dso_local_equivalent` (relative vtables)"),
    row("init:int-of-addr", ON, todo(X64), "an address in an integer field"),
    a64("init:dso-local-equivalent", LATER, ""),
    a64("init:no-cfi", LATER, ""),
];

const FUNCTION: [SupportRow; 6] = [
    row("function:parameters", ON, todo(X64), "a body with parameters; per class of parameter"),
    row("function:variadic", ON, todo(X64), "a variadic function with a body"),
    row(
        "function:may-unwind",
        ON,
        todo(X64),
        "a body without `nounwind`: it needs unwind tables (WP-049)",
    ),
    row("function:personality", ON, todo(X64), "a body with a personality function"),
    row("function:slots", ON, todo(X64), "a body with fixed stack objects (`Body::slots`)"),
    row("function:over-aligned-slot", ON, todo(X64), "a stack object aligned above 16 bytes"),
];

/// In the order of [`cc_index`].
const CC: [SupportRow; 15] = [
    row("cc:ccc", ON, todo(X64), ""),
    row("cc:fastcc", ON, todo(X64), "assigned as `ccc` (`spec/abi-a64.md` §1.2)"),
    a64("cc:coldcc", LATER, ""),
    a64("cc:ghccc", LATER, ""),
    a64("cc:anyregcc", LATER, ""),
    a64("cc:preserve_mostcc", LATER, ""),
    a64("cc:preserve_allcc", LATER, ""),
    row("cc:swiftcc", ON, todo(X64), "assigned as `ccc` (`spec/abi-a64.md` §1.2)"),
    row("cc:cxx_fast_tlscc", ON, todo(X64), "assigned as `ccc` (`spec/abi-a64.md` §1.2)"),
    a64("cc:tailcc", LATER, ""),
    both("cc:cfguard_checkcc", never(WINDOWS), ""),
    a64("cc:swifttailcc", LATER, ""),
    a64("cc:preserve_nonecc", LATER, ""),
    row("cc:aarch64_vector_pcs", todo(LATER), never("an AArch64 convention"), ""),
    row("cc:aarch64_sve_vector_pcs", never(SCALABLE), never("an AArch64 convention"), ""),
];

/// The position in [`CC`] of the convention with LLVM's number `id`.
const fn cc_index(id: u16) -> Option<usize> {
    Some(match id {
        0 => 0,
        8 => 1,
        9 => 2,
        10 => 3,
        13 => 4,
        14 => 5,
        15 => 6,
        16 => 7,
        17 => 8,
        18 => 9,
        19 => 10,
        20 => 11,
        21 => 12,
        97 => 13,
        98 => 14,
        _ => return None,
    })
}

/// In the bit order of the `ParamAttrs` flags, from `ZEROEXT`.
const ATTR_PARAM: [SupportRow; 7] = [
    row("attr:zeroext", ON, todo(X64), ""),
    row("attr:signext", ON, todo(X64), ""),
    a64("attr:inreg", LATER, ""),
    row("attr:nest", ON, todo(X64), ""),
    a64("attr:swiftself", LATER, ""),
    a64("attr:swifterror", LATER, ""),
    a64("attr:swiftasync", LATER, ""),
];

/// In `ParamMem` order, from `Sret`.
const ATTR_MEM: [SupportRow; 6] = [
    row("attr:sret", ON, todo(X64), ""),
    row("attr:byval", ON, todo(X64), ""),
    row("attr:byref", ON, todo(X64), ""),
    both("attr:inalloca", never(WINDOWS), ""),
    both("attr:preallocated", never(WINDOWS), ""),
    a64("attr:elementtype", LATER, "on an inline-assembly or intrinsic argument"),
];

const ATTR_VALUE: [SupportRow; 3] = [
    both("attr:align", ON, "on a parameter or return: an assumption, no code"),
    row(
        "attr:alignstack",
        ON,
        todo(X64),
        "on a parameter: placement of a stack block (abi-a64.md §3.3); on a function: no code, `sp` is always 16-aligned and an over-aligned slot aligns itself",
    ),
    row("attr:uwtable", ON, todo(X64), "on a function with a body (WP-049)"),
];

/// In the order of `FnAttrs::FLAG_NAMES`.
const ATTR_FLAG: [SupportRow; FnAttrs::FLAG_NAMES.len()] = [
    both("attr:noreturn", ON, "a promise: nothing to do in the function itself"),
    both(
        "attr:naked",
        never("a naked function is its inline assembly, with no frame (`spec/codegen.md` §1)"),
        "",
    ),
    both("attr:nounwind", ON, "its absence is `function:may-unwind`"),
    row("attr:returns_twice", ON, todo(X64), "on a body, a call, or a function called directly"),
    both("attr:alwaysinline", ON, "by itself; see `linkage:available_externally`"),
    a64("attr:ssp", "spec/codegen.md §13", "open question: accept and ignore, or implement"),
    a64("attr:sspreq", "spec/codegen.md §13", ""),
    a64("attr:sspstrong", "spec/codegen.md §13", ""),
    a64("attr:presplitcoroutine", CORO, ""),
    a64("attr:sanitize_address", LATER, ""),
    a64("attr:sanitize_alloc_token", LATER, ""),
    a64("attr:sanitize_hwaddress", LATER, ""),
    a64("attr:sanitize_memory", LATER, ""),
    a64("attr:sanitize_memtag", LATER, ""),
    a64("attr:sanitize_numerical_stability", LATER, ""),
    a64("attr:sanitize_realtime", LATER, ""),
    a64("attr:sanitize_realtime_blocking", LATER, ""),
    a64("attr:sanitize_thread", LATER, ""),
    a64("attr:sanitize_type", LATER, ""),
    both(
        "attr:convergent",
        ON,
        "constrains transformations only, and hod makes none (§5.7); `bundle:convergencectrl` and the `llvm.experimental.convergence.*` intrinsics stay refused",
    ),
];

/// In `FnStr::ALL` order.
const ATTR_STRING: [SupportRow; FnStr::ALL.len()] = [
    row("attr:target-cpu", ON, todo(X64), "AArch64: the code is ARMv8.0, which every CPU runs"),
    row(
        "attr:target-features",
        ON,
        todo(X64),
        "each feature judged by code generation (`spec/codegen.md` §5.5.2)",
    ),
    both("attr:tune-cpu", ON, "tuning only: hod does not tune"),
    both("attr:frame-pointer", ON, "any value: hod always keeps a frame pointer"),
    a64("attr:patchable-function-entry", LATER, ""),
    a64("attr:patchable-function-prefix", LATER, ""),
    a64("attr:function-instrument", LATER, ""),
    row("attr:branch-target-enforcement", todo(LATER), never(A64_ONLY), ""),
    row("attr:sign-return-address", todo(LATER), never(A64_ONLY), ""),
    row("attr:sign-return-address-key", todo(LATER), never(A64_ONLY), ""),
    row("attr:branch-protection-pauth-lr", todo(LATER), never(A64_ONLY), ""),
    row("attr:guarded-control-stack", todo(LATER), never(A64_ONLY), ""),
    a64("attr:stack-probe-size", LATER, ""),
    a64("attr:probe-stack", LATER, ""),
    a64("attr:split-stack", LATER, ""),
    a64("attr:zero-call-used-regs", LATER, ""),
    both("attr:stack-protector-buffer-size", ON, "a parameter of `attr:ssp`, read with it"),
    row("attr:min-legal-vector-width", ON, todo(X64), "read by x86 only"),
    row("attr:no-jump-tables", ON, todo(X64), "hod emits no jump table (`spec/codegen.md` §6.2)"),
    both("attr:no-builtins", ON, "hod recognises no library call"),
    both("attr:no-builtin", ON, "every `no-builtin-<function>`; hod recognises no library call"),
];

/// In `TypeClass::ALL` order.
const TYPE: [SupportRow; TypeClass::ALL.len()] = [
    row("type:int", ON, todo(X64), "integers of at most 64 bits"),
    row("type:i128", ON, todo(X64), "WP-040"),
    row("type:int-wide", ON, todo(X64), "integers wider than 64 bits other than `i128` (WP-040)"),
    row("type:ptr", ON, todo(X64), "pointers in address space 0"),
    row("type:half", ON, todo(X64), "arithmetic in single precision"),
    a64("type:bfloat", LATER, ""),
    row("type:float", ON, todo(X64), ""),
    row("type:double", ON, todo(X64), ""),
    row("type:fp128", ON, todo(X64), "arithmetic, compares and conversions are calls"),
    row("type:x86_fp80", never("not an AArch64 type"), todo(X64), ""),
    row(
        "type:vector",
        ON,
        todo(X64),
        "vectors of fixed length; what is lowered of them by element and shape: `spec/codegen.md` §5.5.1 (WP-079)",
    ),
    both("type:scalable-vector", never(SCALABLE), ""),
    row("type:aggregate", ON, todo(X64), "structs and arrays as first-class values (WP-040)"),
    a64("type:token", CORO, "the results of the coroutine intrinsics"),
    both("type:target-ext", never("no target extension type has a lowering"), ""),
];

/// In `Opcode::ALL` order.
const INST: [SupportRow; Opcode::ALL.len()] = [
    row(
        "inst:add",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "inst:sub",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "inst:mul",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "inst:udiv",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "wider than 64 bits: up to 128 (WP-040)",
    ),
    row(
        "inst:sdiv",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "wider than 64 bits: up to 128 (WP-040)",
    ),
    row(
        "inst:urem",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "wider than 64 bits: up to 128 (WP-040)",
    ),
    row(
        "inst:srem",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "wider than 64 bits: up to 128 (WP-040)",
    ),
    row(
        "inst:shl",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "inst:lshr",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "inst:ashr",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "inst:and",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "inst:or",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "inst:xor",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "inst:fadd",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:fsub",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:fmul",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:fdiv",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:frem",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "a call of `fmod`",
    ),
    row(
        "inst:fneg",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:icmp",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Ptr,
            TypeClass::Vector,
        ]),
        todo(X64),
        "vectors: masks of up to 64 lanes (WP-081)",
    ),
    row(
        "inst:fcmp",
        on(&[
            TypeClass::Int,
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "vectors: masks of up to 64 lanes (WP-081)",
    ),
    row(
        "inst:trunc",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "inst:zext",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "inst:sext",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "inst:fptrunc",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:fpext",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:fptoui",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:fptosi",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:uitofp",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:sitofp",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:ptrtoint",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Ptr,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:ptrtoaddr",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Ptr,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:inttoptr",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Ptr,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:bitcast",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Ptr,
            TypeClass::Half,
            TypeClass::BFloat,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "between integers and floats with WP-039",
    ),
    both(
        "inst:addrspacecast",
        never("it has a pointer in a non-zero address space on one side"),
        "",
    ),
    row(
        "inst:load",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Ptr,
            TypeClass::Half,
            TypeClass::BFloat,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Aggregate,
            TypeClass::Vector,
        ]),
        todo(X64),
        "a plain load; see `mem:`; FP and vector values: WP-067",
    ),
    row(
        "inst:store",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Ptr,
            TypeClass::Half,
            TypeClass::BFloat,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Aggregate,
            TypeClass::Vector,
        ]),
        todo(X64),
        "a plain store; see `mem:`; FP and vector values: WP-067",
    ),
    row("inst:alloca", ON, todo(X64), "a dynamic allocation; fixed ones are `function:slots`"),
    row("inst:fence", ON, todo(X64), "WP-074"),
    row(
        "inst:cmpxchg",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Ptr, TypeClass::Aggregate]),
        todo(X64),
        "WP-074; the `{ T, i1 }` result is an aggregate",
    ),
    row(
        "inst:atomicrmw",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::Ptr,
            TypeClass::Half,
            TypeClass::BFloat,
            TypeClass::Float,
            TypeClass::Double,
        ]),
        todo(X64),
        "and the operation's `atomicrmw:` row; WP-074",
    ),
    row("inst:ptradd", ON, todo(X64), "LLVM's `getelementptr` (§5.4); not a vector of pointers"),
    row(
        "inst:extractvalue",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Ptr,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Half,
            TypeClass::Fp128,
            TypeClass::Aggregate,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:insertvalue",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Ptr,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Half,
            TypeClass::Fp128,
            TypeClass::Aggregate,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:extractelement",
        on(&[
            TypeClass::Int,
            TypeClass::Ptr,
            TypeClass::Half,
            TypeClass::BFloat,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Vector,
        ]),
        todo(X64),
        "byte-sized elements, up to 256 lanes (WP-079)",
    ),
    row(
        "inst:insertelement",
        on(&[
            TypeClass::Int,
            TypeClass::Ptr,
            TypeClass::Half,
            TypeClass::BFloat,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Vector,
        ]),
        todo(X64),
        "byte-sized elements, up to 256 lanes (WP-079)",
    ),
    row(
        "inst:shufflevector",
        on(&[TypeClass::Vector]),
        todo(X64),
        "byte-sized elements, up to 256 lanes (WP-079)",
    ),
    row("inst:phi", ON, todo(X64), "for every class whose `type:*` row is supported"),
    row(
        "inst:select",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Ptr,
            TypeClass::Half,
            TypeClass::BFloat,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Aggregate,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "inst:freeze",
        on(&[
            TypeClass::Int,
            TypeClass::I128,
            TypeClass::IntWide,
            TypeClass::Ptr,
            TypeClass::Half,
            TypeClass::BFloat,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Aggregate,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    both(
        "inst:va_arg",
        never("`llc` miscompiles it on AArch64, so there is no oracle; frontends do not emit it"),
        "",
    ),
    row("inst:call", ON, todo(X64), "a call of anything but an intrinsic"),
    row("inst:landingpad", ON, todo(X64), ""),
    both("inst:catchpad", never(WINDOWS_EH), ""),
    both("inst:cleanuppad", never(WINDOWS_EH), ""),
    row("inst:br", ON, todo(X64), ""),
    row("inst:condbr", ON, todo(X64), "LLVM's two-way `br`"),
    row("inst:switch", on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide]), todo(X64), ""),
    row("inst:indirectbr", ON, todo(X64), ""),
    row("inst:ret", ON, todo(X64), ""),
    row("inst:unreachable", ON, todo(X64), ""),
    both(
        "inst:callbr",
        never("its only use is inline assembly with labels (`spec/codegen.md` §1)"),
        "",
    ),
    row("inst:invoke", ON, todo(X64), ""),
    row("inst:resume", ON, todo(X64), ""),
    both("inst:catchswitch", never(WINDOWS_EH), ""),
    both("inst:catchret", never(WINDOWS_EH), ""),
    both("inst:cleanupret", never(WINDOWS_EH), ""),
];

const MEM: [SupportRow; 3] = [
    row("mem:volatile", ON, todo(X64), "a volatile `load`, `store`, `cmpxchg` or `atomicrmw`"),
    row("mem:atomic-load", ON, todo(X64), "WP-074; `fp128` and vectors: `a64:atomic-type`"),
    row("mem:atomic-store", ON, todo(X64), "WP-074; `fp128` and vectors: `a64:atomic-type`"),
];

/// In `RmwOp::ALL` order.
const ATOMICRMW: [SupportRow; RmwOp::ALL.len()] = [
    row("atomicrmw:xchg", ON, todo(X64), "WP-074"),
    row("atomicrmw:add", ON, todo(X64), "WP-074"),
    row("atomicrmw:sub", ON, todo(X64), "WP-074"),
    row("atomicrmw:and", ON, todo(X64), "WP-074"),
    row("atomicrmw:nand", ON, todo(X64), "WP-074"),
    row("atomicrmw:or", ON, todo(X64), "WP-074"),
    row("atomicrmw:xor", ON, todo(X64), "WP-074"),
    row("atomicrmw:max", ON, todo(X64), "WP-074"),
    row("atomicrmw:min", ON, todo(X64), "WP-074"),
    row("atomicrmw:umax", ON, todo(X64), "WP-074"),
    row("atomicrmw:umin", ON, todo(X64), "WP-074"),
    row("atomicrmw:fadd", ON, todo(X64), "WP-074"),
    row("atomicrmw:fsub", ON, todo(X64), "WP-074"),
    row("atomicrmw:fmax", ON, todo(X64), "WP-074"),
    row("atomicrmw:fmin", ON, todo(X64), "WP-074"),
    row("atomicrmw:uinc_wrap", ON, todo(X64), "WP-074"),
    row("atomicrmw:udec_wrap", ON, todo(X64), "WP-074"),
    row("atomicrmw:usub_cond", ON, todo(X64), "WP-074"),
    row("atomicrmw:usub_sat", ON, todo(X64), "WP-074"),
    row("atomicrmw:fmaximum", ON, todo(X64), "WP-074"),
    row("atomicrmw:fminimum", ON, todo(X64), "WP-074"),
];

const CALL: [SupportRow; 4] = [
    row("call:indirect", ON, todo(X64), "the callee is a computed address"),
    a64("call:inline-asm", ASM, "the callee is inline assembly an assembler must read"),
    row(
        "call:inline-asm-empty",
        ON,
        todo(X64),
        "the callee is inline assembly whose template assembles to nothing (`spec/ir.md` §2.3)",
    ),
    row("call:variadic", ON, todo(X64), "a call through a variadic function type"),
];

const TAIL: [SupportRow; 3] = [
    both("tail:tail", ON, "a permission: ignored (`spec/codegen.md` §7.1)"),
    both("tail:notail", ON, "hod makes no tail call of its own"),
    row(
        "tail:musttail",
        ON,
        todo(X64),
        "and the `musttail:` rules of §8.6, which need the `StackArgs` hook (`hod_cg::abi::TargetStackArgs`)",
    ),
];

const BUNDLE: [SupportRow; 10] = [
    a64("bundle:deopt", LATER, ""),
    both("bundle:funclet", never(WINDOWS_EH), ""),
    both("bundle:gc-transition", never(GC), ""),
    both("bundle:gc-live", never(GC), ""),
    both("bundle:cfguardtarget", never(WINDOWS), ""),
    both("bundle:preallocated", never(WINDOWS), ""),
    a64("bundle:clang.arc.attachedcall", LATER, ""),
    a64("bundle:ptrauth", LATER, ""),
    a64("bundle:kcfi", LATER, ""),
    a64("bundle:convergencectrl", LATER, ""),
];

const LANDINGPAD: [SupportRow; 3] = [
    row("landingpad:cleanup", ON, todo(X64), ""),
    row("landingpad:catch", ON, todo(X64), ""),
    row("landingpad:filter", ON, todo(X64), ""),
];

const OPERAND: [SupportRow; 9] = [
    row(
        "operand:symbol",
        ON,
        todo(X64),
        "a symbol's address as a value, with or without an offset",
    ),
    row("operand:slot", ON, todo(X64), "the address of a stack slot"),
    row("operand:blockaddress", ON, todo(X64), "of a block of the same function"),
    row("operand:addr-of-int", ON, todo(X64), "`inttoptr` of a constant number"),
    row("operand:int-of-addr", ON, todo(X64), "`ptrtoint` of a constant address (WP-072)"),
    row("operand:sym-diff", ON, todo(X64), "a difference of two symbol addresses (WP-072)"),
    a64("operand:dso-local-equivalent", LATER, ""),
    a64("operand:no-cfi", LATER, ""),
    row(
        "operand:address-in-aggregate",
        ON,
        todo(X64),
        "an aggregate or vector constant with an address (WP-072)",
    ),
];

/// In the order of [`const_op_index`].
const CONST_EXPR: [SupportRow; 19] = [
    both("const-expr:trunc", never(NO_FORM), ""),
    both("const-expr:zext", never(NO_FORM), ""),
    both("const-expr:sext", never(NO_FORM), ""),
    both("const-expr:ptrtoint", never(NO_FORM), ""),
    both("const-expr:ptrtoaddr", never(NO_FORM), ""),
    both("const-expr:inttoptr", never(NO_FORM), ""),
    both("const-expr:bitcast", never(NO_FORM), ""),
    both("const-expr:addrspacecast", never(NO_FORM), ""),
    both("const-expr:getelementptr", never(NO_FORM), ""),
    both("const-expr:add", never(NO_FORM), ""),
    both("const-expr:sub", never(NO_FORM), ""),
    both("const-expr:mul", never(NO_FORM), ""),
    both("const-expr:xor", never(NO_FORM), ""),
    both("const-expr:and", never(NO_FORM), ""),
    both("const-expr:or", never(NO_FORM), ""),
    both("const-expr:shl", never(NO_FORM), ""),
    both("const-expr:extractelement", never(NO_FORM), ""),
    both("const-expr:insertelement", never(NO_FORM), ""),
    both("const-expr:shufflevector", never(NO_FORM), ""),
];

/// The position in [`CONST_EXPR`] of `op`. Exhaustive on purpose.
const fn const_op_index(op: ConstOp) -> usize {
    match op {
        ConstOp::Trunc => 0,
        ConstOp::ZExt => 1,
        ConstOp::SExt => 2,
        ConstOp::PtrToInt => 3,
        ConstOp::PtrToAddr => 4,
        ConstOp::IntToPtr => 5,
        ConstOp::BitCast => 6,
        ConstOp::AddrSpaceCast => 7,
        ConstOp::GetElementPtr => 8,
        ConstOp::Add => 9,
        ConstOp::Sub => 10,
        ConstOp::Mul => 11,
        ConstOp::Xor => 12,
        ConstOp::And => 13,
        ConstOp::Or => 14,
        ConstOp::Shl => 15,
        ConstOp::ExtractElement => 16,
        ConstOp::InsertElement => 17,
        ConstOp::ShuffleVector => 18,
    }
}

/// In `Intrinsic::ALL` order.
const INTRINSIC: [SupportRow; Intrinsic::ALL.len()] = [
    row("intrinsic:llvm.memcpy", on(&[TypeClass::Ptr, TypeClass::Int]), todo(X64), ""),
    row("intrinsic:llvm.memcpy.inline", on(&[TypeClass::Ptr, TypeClass::Int]), todo(X64), ""),
    row("intrinsic:llvm.memmove", on(&[TypeClass::Ptr, TypeClass::Int]), todo(X64), ""),
    row("intrinsic:llvm.memset", on(&[TypeClass::Ptr, TypeClass::Int]), todo(X64), ""),
    row("intrinsic:llvm.memset.inline", on(&[TypeClass::Ptr, TypeClass::Int]), todo(X64), ""),
    row("intrinsic:llvm.va_start", ON, todo(X64), ""),
    row("intrinsic:llvm.va_end", ON, todo(X64), ""),
    row("intrinsic:llvm.va_copy", ON, todo(X64), ""),
    row("intrinsic:llvm.stacksave", ON, todo(X64), ""),
    row("intrinsic:llvm.stackrestore", ON, todo(X64), ""),
    row("intrinsic:llvm.frameaddress", on(&[TypeClass::Ptr, TypeClass::Int]), todo(X64), ""),
    row("intrinsic:llvm.returnaddress", on(&[TypeClass::Ptr, TypeClass::Int]), todo(X64), ""),
    row("intrinsic:llvm.trap", ON, todo(X64), ""),
    row("intrinsic:llvm.debugtrap", ON, todo(X64), ""),
    row("intrinsic:llvm.ubsantrap", on(&[TypeClass::Int]), todo(X64), ""),
    row(
        "intrinsic:llvm.bswap",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.bitreverse",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.ctpop",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.ctlz",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.cttz",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.fshl",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.fshr",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.abs",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.smax",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.smin",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.umax",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.umin",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.scmp",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.ucmp",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    a64("intrinsic:llvm.clmul", LATER, ""),
    row(
        "intrinsic:llvm.sadd.with.overflow",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Aggregate]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.uadd.with.overflow",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Aggregate]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.ssub.with.overflow",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Aggregate]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.usub.with.overflow",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Aggregate]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.smul.with.overflow",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Aggregate]),
        todo(X64),
        "wider than 2048 bits refused (`a64:wide-mul-overflow`)",
    ),
    row(
        "intrinsic:llvm.umul.with.overflow",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::IntWide, TypeClass::Aggregate]),
        todo(X64),
        "wider than 2048 bits refused (`a64:wide-mul-overflow`)",
    ),
    row(
        "intrinsic:llvm.sadd.sat",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.uadd.sat",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.ssub.sat",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.usub.sat",
        on(&[TypeClass::Int, TypeClass::I128, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.fabs",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.sqrt",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.floor",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.ceil",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.trunc",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.round",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.roundeven",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.nearbyint",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.rint",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.copysign",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.fma",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.fmuladd",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.minnum",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.maxnum",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.minimum",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.maximum",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.minimumnum",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.maximumnum",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.canonicalize",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.arithmetic.fence",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.pow",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.powi",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Int,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.ldexp",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Int,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.frexp",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Int,
            TypeClass::Aggregate,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.sin",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.cos",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.tan",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.asin",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.acos",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.atan",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.atan2",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.sinh",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.cosh",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.tanh",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.exp",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.exp2",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.exp10",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.log",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.log2",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.log10",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.lround",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Int,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.llround",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Int,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.lrint",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Int,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.llrint",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Int,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.fptosi.sat",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Int,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.fptoui.sat",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Int,
            TypeClass::Vector,
        ]),
        todo(X64),
        "",
    ),
    row(
        "intrinsic:llvm.is.fpclass",
        on(&[
            TypeClass::Half,
            TypeClass::Float,
            TypeClass::Double,
            TypeClass::Fp128,
            TypeClass::Int,
        ]),
        todo(X64),
        "",
    ),
    row("intrinsic:llvm.eh.typeid.for", on(&[TypeClass::Int, TypeClass::Ptr]), todo(X64), ""),
    row("intrinsic:llvm.threadlocal.address", on(&[TypeClass::Ptr]), todo(X64), ""),
    row("intrinsic:llvm.thread.pointer", on(&[TypeClass::Ptr]), todo(X64), ""),
    row("intrinsic:llvm.prefetch", on(&[TypeClass::Ptr, TypeClass::Int]), todo(X64), ""),
    row("intrinsic:llvm.clear_cache", on(&[TypeClass::Ptr]), todo(X64), ""),
    row("intrinsic:llvm.readcyclecounter", on(&[TypeClass::Int]), todo(X64), ""),
    row("intrinsic:llvm.is.constant", ON, todo(X64), ""),
    row("intrinsic:llvm.objectsize", on(&[TypeClass::Ptr, TypeClass::Int]), todo(X64), ""),
    row(
        "intrinsic:llvm.ptrmask",
        on(&[TypeClass::Ptr, TypeClass::Int, TypeClass::Vector]),
        todo(X64),
        "",
    ),
    a64("intrinsic:llvm.load.relative", LATER, ""),
    a64("intrinsic:llvm.instrprof.increment", LATER, ""),
    a64("intrinsic:llvm.instrprof.increment.step", LATER, ""),
    row(
        "intrinsic:llvm.vector.reduce.add",
        on(&[TypeClass::Int, TypeClass::Vector]),
        todo(X64),
        "masks of up to 64 lanes, lanes up to 256 (WP-081)",
    ),
    row(
        "intrinsic:llvm.vector.reduce.mul",
        on(&[TypeClass::Int, TypeClass::Vector]),
        todo(X64),
        "masks of up to 64 lanes, lanes up to 256 (WP-081)",
    ),
    row(
        "intrinsic:llvm.vector.reduce.and",
        on(&[TypeClass::Int, TypeClass::Vector]),
        todo(X64),
        "masks of up to 64 lanes, lanes up to 256 (WP-081)",
    ),
    row(
        "intrinsic:llvm.vector.reduce.or",
        on(&[TypeClass::Int, TypeClass::Vector]),
        todo(X64),
        "masks of up to 64 lanes, lanes up to 256 (WP-081)",
    ),
    row(
        "intrinsic:llvm.vector.reduce.xor",
        on(&[TypeClass::Int, TypeClass::Vector]),
        todo(X64),
        "masks of up to 64 lanes, lanes up to 256 (WP-081)",
    ),
    row(
        "intrinsic:llvm.vector.reduce.smax",
        on(&[TypeClass::Int, TypeClass::Vector]),
        todo(X64),
        "masks of up to 64 lanes, lanes up to 256 (WP-081)",
    ),
    row(
        "intrinsic:llvm.vector.reduce.smin",
        on(&[TypeClass::Int, TypeClass::Vector]),
        todo(X64),
        "masks of up to 64 lanes, lanes up to 256 (WP-081)",
    ),
    row(
        "intrinsic:llvm.vector.reduce.umax",
        on(&[TypeClass::Int, TypeClass::Vector]),
        todo(X64),
        "masks of up to 64 lanes, lanes up to 256 (WP-081)",
    ),
    row(
        "intrinsic:llvm.vector.reduce.umin",
        on(&[TypeClass::Int, TypeClass::Vector]),
        todo(X64),
        "masks of up to 64 lanes, lanes up to 256 (WP-081)",
    ),
    row(
        "intrinsic:llvm.vector.reduce.fmax",
        on(&[TypeClass::Half, TypeClass::Float, TypeClass::Double, TypeClass::Vector]),
        todo(X64),
        "lanes up to 256 (WP-081)",
    ),
    row(
        "intrinsic:llvm.vector.reduce.fmin",
        on(&[TypeClass::Half, TypeClass::Float, TypeClass::Double, TypeClass::Vector]),
        todo(X64),
        "lanes up to 256 (WP-081)",
    ),
    row(
        "intrinsic:llvm.vector.reduce.fadd",
        on(&[TypeClass::Half, TypeClass::Float, TypeClass::Double, TypeClass::Vector]),
        todo(X64),
        "lanes up to 256 (WP-081)",
    ),
    row(
        "intrinsic:llvm.vector.reduce.fmul",
        on(&[TypeClass::Half, TypeClass::Float, TypeClass::Double, TypeClass::Vector]),
        todo(X64),
        "lanes up to 256 (WP-081)",
    ),
    a64("intrinsic:llvm.vector.extract", VECTORS, ""),
    a64("intrinsic:llvm.vector.insert", VECTORS, ""),
    a64("intrinsic:llvm.coro.id", CORO, ""),
    a64("intrinsic:llvm.coro.alloc", CORO, ""),
    a64("intrinsic:llvm.coro.begin", CORO, ""),
    a64("intrinsic:llvm.coro.end", CORO, ""),
    a64("intrinsic:llvm.coro.free", CORO, ""),
    a64("intrinsic:llvm.coro.size", CORO, ""),
    a64("intrinsic:llvm.coro.align", CORO, ""),
    a64("intrinsic:llvm.coro.save", CORO, ""),
    a64("intrinsic:llvm.coro.suspend", CORO, ""),
    a64("intrinsic:llvm.coro.resume", CORO, ""),
    a64("intrinsic:llvm.coro.destroy", CORO, ""),
    a64("intrinsic:llvm.coro.done", CORO, ""),
    a64("intrinsic:llvm.coro.promise", CORO, ""),
    a64("intrinsic:llvm.coro.frame", CORO, ""),
    a64("intrinsic:llvm.coro.noop", CORO, ""),
    a64("intrinsic:llvm.coro.is_in_ramp", CORO, ""),
    a64("intrinsic:llvm.coro.await.suspend.void", CORO, ""),
    a64("intrinsic:llvm.coro.await.suspend.bool", CORO, ""),
    a64("intrinsic:llvm.coro.await.suspend.handle", CORO, ""),
    row("intrinsic:llvm.sshl.sat", on(&[TypeClass::Int]), todo(X64), "up to 64 bits (WP-084)"),
    row("intrinsic:llvm.ushl.sat", on(&[TypeClass::Int]), todo(X64), "up to 64 bits (WP-084)"),
    row("intrinsic:llvm.aarch64.crc32b", on(&[TypeClass::Int]), never(A64_ONLY), ""),
    row("intrinsic:llvm.aarch64.crc32h", on(&[TypeClass::Int]), never(A64_ONLY), ""),
    row("intrinsic:llvm.aarch64.crc32w", on(&[TypeClass::Int]), never(A64_ONLY), ""),
    row("intrinsic:llvm.aarch64.crc32x", on(&[TypeClass::Int]), never(A64_ONLY), ""),
    row("intrinsic:llvm.aarch64.crc32cb", on(&[TypeClass::Int]), never(A64_ONLY), ""),
    row("intrinsic:llvm.aarch64.crc32ch", on(&[TypeClass::Int]), never(A64_ONLY), ""),
    row("intrinsic:llvm.aarch64.crc32cw", on(&[TypeClass::Int]), never(A64_ONLY), ""),
    row("intrinsic:llvm.aarch64.crc32cx", on(&[TypeClass::Int]), never(A64_ONLY), ""),
];

// ---- the table ----

const EMPTY: SupportRow = both("", ON, "");

/// The number of matrix rows.
pub(crate) const ROW_COUNT: usize = MODULE.len()
    + STRUCTOR.len()
    + DEBUG_ROWS.len()
    + GLOBAL.len()
    + LINKAGE.len()
    + VISIBILITY.len()
    + TLS_ROWS.len()
    + COMDAT.len()
    + INIT.len()
    + FUNCTION.len()
    + CC.len()
    + ATTR_PARAM.len()
    + ATTR_MEM.len()
    + ATTR_VALUE.len()
    + ATTR_FLAG.len()
    + ATTR_STRING.len()
    + TYPE.len()
    + INST.len()
    + MEM.len()
    + ATOMICRMW.len()
    + CALL.len()
    + TAIL.len()
    + BUNDLE.len()
    + LANDINGPAD.len()
    + OPERAND.len()
    + CONST_EXPR.len()
    + INTRINSIC.len();

/// The support matrix (§8.2): every construct that has a row, grouped as
/// [`SUPPORT_GROUPS`] lists the groups.
pub static SUPPORT_ROWS: [SupportRow; ROW_COUNT] = {
    let mut out = [EMPTY; ROW_COUNT];
    let mut at = 0;
    macro_rules! append {
        ($($group:ident),+) => {$(
            let mut index = 0;
            while index < $group.len() {
                out[at] = $group[index];
                at += 1;
                index += 1;
            }
        )+};
    }
    append!(
        MODULE,
        STRUCTOR,
        DEBUG_ROWS,
        GLOBAL,
        LINKAGE,
        VISIBILITY,
        TLS_ROWS,
        COMDAT,
        INIT,
        FUNCTION,
        CC,
        ATTR_PARAM,
        ATTR_MEM,
        ATTR_VALUE,
        ATTR_FLAG,
        ATTR_STRING,
        TYPE,
        INST,
        MEM,
        ATOMICRMW,
        CALL,
        TAIL,
        BUNDLE,
        LANDINGPAD,
        OPERAND,
        CONST_EXPR,
        INTRINSIC
    );
    assert!(at == ROW_COUNT);
    out
};

/// The groups of [`SUPPORT_ROWS`] in table order: the prefix of the
/// names and what the group holds.
pub const SUPPORT_GROUPS: [(&str, &str); 23] = [
    ("module", "Module properties (`ModuleProps`, §2.1) and module-level tables"),
    ("structor", "Constructor and destructor entries"),
    ("debug", "Debug information"),
    ("global", "Symbols: what they are and what a definition carries"),
    ("linkage", "Linkages, on declarations and definitions"),
    ("visibility", "Visibilities other than `default`"),
    ("tls", "Thread-local models of variables and aliases"),
    ("comdat", "COMDAT selection kinds of a definition's COMDAT"),
    ("init", "What a variable's initializer contains"),
    ("function", "Properties of a function that has a body"),
    ("cc", "Calling conventions, of a body's function and of a call"),
    ("attr", "Kept attributes (§5.7) of parameters, returns, functions and calls"),
    ("type", "Type classes of the values instructions produce and consume"),
    ("inst", "Instructions"),
    ("mem", "Kinds of memory access"),
    ("atomicrmw", "`atomicrmw` operations"),
    ("call", "Kinds of callee"),
    ("tail", "Tail-call markers"),
    ("bundle", "Operand bundles, by tag"),
    ("landingpad", "Landing-pad clauses"),
    ("operand", "Constant addresses as instruction operands"),
    ("const-expr", "Constant expressions without a normal form, in initializers and operands"),
    ("intrinsic", "Intrinsics (§5.5)"),
];

const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut index = 0;
    while index < a.len() {
        if a[index] != b[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// The index of the row called `name`. Evaluated at compile time for
/// every row the classifier names, so a renamed row cannot be missed.
const fn find(name: &str) -> usize {
    let mut index = 0;
    while index < SUPPORT_ROWS.len() {
        if str_eq(SUPPORT_ROWS[index].name, name) {
            return index;
        }
        index += 1;
    }
    panic!("the support matrix has no such row");
}

/// The index of the row called `name`.
pub(crate) fn row_index(name: &str) -> Option<usize> {
    SUPPORT_ROWS.iter().position(|row| row.name == name)
}

/// Row indices, for the classifier.
pub(crate) mod at {
    use super::{cc_index, const_op_index, find};
    use crate::constant::ConstOp;

    pub(crate) const MODULE_PIC: usize = find("module:pic");
    pub(crate) const MODULE_PIE: usize = find("module:pie");
    /// Plus `CodeModel as usize`.
    pub(crate) const MODULE_CODE_MODEL: usize = find("module:code-model-tiny");
    pub(crate) const MODULE_LARGE_DATA: usize = find("module:large-data-threshold");
    pub(crate) const MODULE_UWTABLE: usize = find("module:uwtable");
    pub(crate) const MODULE_FRAME_POINTER: usize = find("module:frame-pointer");
    /// Plus the bit number of a `BranchProtection` bit.
    pub(crate) const MODULE_PROTECTION: usize = find("module:branch-target-enforcement");
    pub(crate) const MODULE_ASM: usize = find("module:asm");
    pub(crate) const MODULE_ASM_SYMBOLS: usize = find("module:asm-symbols");
    pub(crate) const MODULE_LINKER_OPTIONS: usize = find("module:linker-options");
    pub(crate) const MODULE_DEPENDENT_LIBRARIES: usize = find("module:dependent-libraries");
    pub(crate) const MODULE_CTORS: usize = find("module:global-ctors");
    pub(crate) const MODULE_DTORS: usize = find("module:global-dtors");
    pub(crate) const STRUCTOR_PRIORITY: usize = find("structor:priority");
    pub(crate) const STRUCTOR_ASSOCIATED: usize = find("structor:associated");

    pub(crate) const DEBUG_MODULE_FLAGS: usize = find("debug:module-flags");
    pub(crate) const DEBUG_SUBPROGRAM: usize = find("debug:subprogram");
    pub(crate) const DEBUG_SLOT_VARIABLES: usize = find("debug:slot-variables");
    pub(crate) const DEBUG_LOCATIONS: usize = find("debug:locations");

    pub(crate) const FUNCTION_DECLARATION: usize = find("global:function-declaration");
    pub(crate) const VARIABLE_DECLARATION: usize = find("global:variable-declaration");
    pub(crate) const FUNCTION_DEFINITION: usize = find("global:function-definition");
    pub(crate) const VARIABLE_DEFINITION: usize = find("global:variable-definition");
    pub(crate) const ALIAS: usize = find("global:alias");
    pub(crate) const ALIAS_OFFSET: usize = find("global:alias-offset");
    pub(crate) const ALIAS_ABSOLUTE: usize = find("global:alias-absolute");
    pub(crate) const IFUNC: usize = find("global:ifunc");
    pub(crate) const SECTION: usize = find("global:section");
    pub(crate) const ALIGN: usize = find("global:align");
    pub(crate) const UNNAMED_ADDR: usize = find("global:unnamed-addr");
    pub(crate) const USED: usize = find("global:used");
    /// Plus `Linkage as usize`.
    pub(crate) const LINKAGE: usize = find("linkage:external");
    /// Plus `Visibility as usize - 1`.
    pub(crate) const VISIBILITY: usize = find("visibility:hidden");
    /// Plus `TlsModel as usize - 1`.
    pub(crate) const TLS: usize = find("tls:general-dynamic");
    /// Plus `ComdatKind as usize`.
    pub(crate) const COMDAT: usize = find("comdat:any");

    pub(crate) const INIT_DATA: usize = find("init:data");
    pub(crate) const INIT_SYMBOL: usize = find("init:symbol");
    pub(crate) const INIT_BLOCKADDRESS: usize = find("init:blockaddress");
    pub(crate) const INIT_SYM_DIFF: usize = find("init:sym-diff");
    pub(crate) const INIT_SYM_DIFF_PLT: usize = find("init:sym-diff-plt");
    pub(crate) const INIT_INT_OF_ADDR: usize = find("init:int-of-addr");
    pub(crate) const INIT_DSO_LOCAL_EQUIVALENT: usize = find("init:dso-local-equivalent");
    pub(crate) const INIT_NO_CFI: usize = find("init:no-cfi");

    pub(crate) const FUNCTION_PARAMETERS: usize = find("function:parameters");
    pub(crate) const FUNCTION_VARIADIC: usize = find("function:variadic");
    pub(crate) const FUNCTION_MAY_UNWIND: usize = find("function:may-unwind");
    pub(crate) const FUNCTION_PERSONALITY: usize = find("function:personality");
    pub(crate) const FUNCTION_SLOTS: usize = find("function:slots");
    pub(crate) const FUNCTION_OVER_ALIGNED_SLOT: usize = find("function:over-aligned-slot");

    const CC: usize = find("cc:ccc");
    /// The row of the calling convention with LLVM's number `id`.
    pub(crate) const fn cc(id: u16) -> Option<usize> {
        match cc_index(id) {
            Some(index) => Some(CC + index),
            None => None,
        }
    }

    /// Plus the position of a `ParamAttrs` flag, from `ZEROEXT`.
    pub(crate) const ATTR_PARAM: usize = find("attr:zeroext");
    /// Plus `ParamMem as usize - 1`.
    pub(crate) const ATTR_MEM: usize = find("attr:sret");
    pub(crate) const ATTR_ALIGN: usize = find("attr:align");
    pub(crate) const ATTR_ALIGNSTACK: usize = find("attr:alignstack");
    pub(crate) const ATTR_UWTABLE: usize = find("attr:uwtable");
    /// Plus the bit number of an `FnAttrs` flag.
    pub(crate) const ATTR_FLAG: usize = find("attr:noreturn");
    pub(crate) const ATTR_RETURNS_TWICE: usize = find("attr:returns_twice");
    /// Plus `FnStr as usize`.
    pub(crate) const ATTR_STRING: usize = find("attr:target-cpu");

    /// Plus `TypeClass as usize`.
    pub(crate) const TYPE: usize = find("type:int");
    /// Plus `Opcode as usize`.
    pub(crate) const INST: usize = find("inst:add");
    pub(crate) const MEM_VOLATILE: usize = find("mem:volatile");
    pub(crate) const MEM_ATOMIC_LOAD: usize = find("mem:atomic-load");
    pub(crate) const MEM_ATOMIC_STORE: usize = find("mem:atomic-store");
    /// Plus `RmwOp as usize`.
    pub(crate) const ATOMICRMW: usize = find("atomicrmw:xchg");
    pub(crate) const CALL_INDIRECT: usize = find("call:indirect");
    pub(crate) const CALL_INLINE_ASM: usize = find("call:inline-asm");
    pub(crate) const CALL_INLINE_ASM_EMPTY: usize = find("call:inline-asm-empty");
    pub(crate) const CALL_VARIADIC: usize = find("call:variadic");
    pub(crate) const TAIL_TAIL: usize = find("tail:tail");
    pub(crate) const TAIL_NOTAIL: usize = find("tail:notail");
    pub(crate) const TAIL_MUSTTAIL: usize = find("tail:musttail");
    /// The first bundle row; the group has `BUNDLE_COUNT` rows.
    pub(crate) const BUNDLE: usize = find("bundle:deopt");
    pub(crate) const BUNDLE_COUNT: usize = super::BUNDLE.len();
    pub(crate) const LANDINGPAD_CLEANUP: usize = find("landingpad:cleanup");
    pub(crate) const LANDINGPAD_CATCH: usize = find("landingpad:catch");
    pub(crate) const LANDINGPAD_FILTER: usize = find("landingpad:filter");

    pub(crate) const OPERAND_SYMBOL: usize = find("operand:symbol");
    pub(crate) const OPERAND_SLOT: usize = find("operand:slot");
    pub(crate) const OPERAND_BLOCKADDRESS: usize = find("operand:blockaddress");
    pub(crate) const OPERAND_ADDR_OF_INT: usize = find("operand:addr-of-int");
    pub(crate) const OPERAND_INT_OF_ADDR: usize = find("operand:int-of-addr");
    pub(crate) const OPERAND_SYM_DIFF: usize = find("operand:sym-diff");
    pub(crate) const OPERAND_DSO_LOCAL_EQUIVALENT: usize = find("operand:dso-local-equivalent");
    pub(crate) const OPERAND_NO_CFI: usize = find("operand:no-cfi");
    pub(crate) const OPERAND_ADDRESS_IN_AGGREGATE: usize = find("operand:address-in-aggregate");

    const CONST_EXPR: usize = find("const-expr:trunc");
    /// The row of constant expression `op`.
    pub(crate) const fn const_expr(op: ConstOp) -> usize {
        CONST_EXPR + const_op_index(op)
    }

    /// Plus the position in `Intrinsic::ALL`.
    pub(crate) const INTRINSIC: usize = find("intrinsic:llvm.memcpy");

    /// Not a row: the index of the mask that accepts every class, for
    /// values whose instruction has no row of its own.
    pub(crate) const ANY: usize = super::ROW_COUNT;
}

// ---- names that are not rows ----

/// The structural rules (§8.6): reasons that no [`Support`] takes away.
/// Rows whose status is `Rejected` on both targets are structural too;
/// they are in [`SUPPORT_ROWS`].
///
/// [`Support`]: crate::Support
pub const STRUCTURAL_RULES: [ConstructInfo; 13] = [
    rule(rules::TLS_OPERAND, "a thread-local symbol as the callee of a call"),
    rule(
        rules::TLS_CONSTANT,
        "the address of a thread-local symbol inside a constant other than a plain address operand (`@t`, `getelementptr (@t, ...)`): an initializer, an aggregate or expression operand, a landing-pad clause",
    ),
    rule(
        rules::BLOCKADDRESS_OTHER,
        "a `blockaddress` in a body that names a block of another function",
    ),
    rule(
        rules::MUSTTAIL_CALLEE_POP,
        "a `musttail` call with a convention whose callee pops its arguments (`tailcc`, `swifttailcc`)",
    ),
    rule(
        rules::MUSTTAIL_GROW,
        "a `musttail` call whose stack-argument area is larger than its caller's",
    ),
    rule(
        rules::MUSTTAIL_UNKNOWN,
        "a `musttail` call whose stack-argument areas are not known: no `StackArgs` was given, or it had no answer",
    ),
    rule(rules::ATOMIC_SCALABLE, "an atomic access to a scalable vector"),
    rule(rules::ADDRSPACE_POINTER, "a value whose type is a pointer in a non-zero address space"),
    rule(rules::ADDRSPACE_GLOBAL, "a variable, alias or IFUNC in a non-zero address space"),
    rule(rules::ADDRSPACE_FUNCTION, "a function in a non-zero address space"),
    rule(rules::ADDRSPACE_ALLOCA, "an `alloca` in a non-zero address space"),
    rule(rules::CONSTANT_NESTING, "a constant nested more than 64 aggregates deep"),
    rule(
        rules::TARGET_MISMATCH,
        "the `Support` given is for another architecture than the module's",
    ),
];

/// The malformed-input outcomes (§8.7): a module or body that is not
/// well formed in a way `Body::verify` and the builders cannot see.
pub const INVALID_RULES: [ConstructInfo; 12] = [
    rule(rules::BODY_SYMBOL, "a body's symbol is not a symbol of the module"),
    rule(rules::BODY_NOT_FUNCTION, "a body's symbol is not a function"),
    rule(rules::BODY_TYPE, "a body's function type is not its symbol's"),
    rule(
        rules::BODY,
        "a body's tables disagree: its argument count is not its type's, or an operand names an instruction, parameter, stack slot or record the body does not have",
    ),
    rule(
        rules::SYMBOL,
        "a symbol id in an operand, a constant, a personality, a resolver or a constructor entry names no symbol; a COMDAT id names no COMDAT",
    ),
    rule(rules::UNRESOLVED_SYMBOL, "a symbol was named and never described"),
    rule(
        rules::INLINE_ASM,
        "an inline-assembly callee names no entry of the module's table, or inline assembly is an operand other than a callee",
    ),
    rule(rules::CONSTANT, "a constant id names no constant"),
    rule(
        rules::TYPE,
        "a value's type is no type, or a type without values; a call's function type is not one",
    ),
    rule(
        rules::BLOCKADDRESS,
        "a `blockaddress` names a symbol that is not a function with a body, or a block its function does not have",
    ),
    rule(
        rules::INITIALIZER,
        "an initializer contains the `none` token, or an alias target is not an address",
    ),
    rule(rules::FN_ATTRS, "a function's attribute set is no set of the module"),
];

/// The names built from the input (§8.5). Everything else `classify`
/// reports is in [`CONSTRUCT_NAMES`].
pub const CONSTRUCT_FAMILIES: [ConstructInfo; 6] = [
    rule(
        "intrinsic:<name>",
        "a call of `Intrinsic::Other(name)`: an `llvm.*` name hod does not know, the no-code names (§5.5) included",
    ),
    rule(
        "module-flag:<name>",
        "a module flag that is neither known nor ignorable (§2.1), or any `Require` flag",
    ),
    rule("cc:<number>", "a calling convention LLVM has no keyword for"),
    rule("bundle:<tag>", "an operand bundle whose tag has no row"),
    rule(
        "<row>/<class>",
        "a supported row met with a value of a class it is not implemented for (§8.3), e.g. `inst:select/double`",
    ),
    rule(
        "<producer record>",
        "a name a producer gave to `Module::record_unsupported` (§2.2), passed through unchanged",
    ),
];

const fn rule(name: &'static str, what: &'static str) -> ConstructInfo {
    ConstructInfo { name, what }
}

/// The names of [`STRUCTURAL_RULES`] and [`INVALID_RULES`].
pub(crate) mod rules {
    pub(crate) const TLS_OPERAND: &str = "tls:direct-operand";
    pub(crate) const TLS_CONSTANT: &str = "tls:address-in-constant";
    pub(crate) const BLOCKADDRESS_OTHER: &str = "blockaddress:other-function";
    pub(crate) const MUSTTAIL_CALLEE_POP: &str = "musttail:callee-pop";
    pub(crate) const MUSTTAIL_GROW: &str = "musttail:stack-args-grow";
    pub(crate) const MUSTTAIL_UNKNOWN: &str = "musttail:stack-args-unknown";
    pub(crate) const ATOMIC_SCALABLE: &str = "atomic:scalable-vector";
    pub(crate) const ADDRSPACE_POINTER: &str = "addrspace:pointer";
    pub(crate) const ADDRSPACE_GLOBAL: &str = "addrspace:global";
    pub(crate) const ADDRSPACE_FUNCTION: &str = "addrspace:function";
    pub(crate) const ADDRSPACE_ALLOCA: &str = "addrspace:alloca";
    pub(crate) const CONSTANT_NESTING: &str = "limit:constant-nesting";
    pub(crate) const TARGET_MISMATCH: &str = "target:support-mismatch";

    pub(crate) const BODY_SYMBOL: &str = "invalid:body-symbol";
    pub(crate) const BODY_NOT_FUNCTION: &str = "invalid:body-not-function";
    pub(crate) const BODY_TYPE: &str = "invalid:body-type";
    pub(crate) const BODY: &str = "invalid:body";
    pub(crate) const SYMBOL: &str = "invalid:symbol";
    pub(crate) const UNRESOLVED_SYMBOL: &str = "invalid:unresolved-symbol";
    pub(crate) const INLINE_ASM: &str = "invalid:inline-asm";
    pub(crate) const CONSTANT: &str = "invalid:constant";
    pub(crate) const TYPE: &str = "invalid:type";
    pub(crate) const BLOCKADDRESS: &str = "invalid:blockaddress";
    pub(crate) const INITIALIZER: &str = "invalid:initializer";
    pub(crate) const FN_ATTRS: &str = "invalid:function-attributes";
}

const NAME_COUNT: usize = ROW_COUNT + STRUCTURAL_RULES.len() + INVALID_RULES.len();

/// Every fixed construct name `classify` can report (§8.5): the rows of
/// [`SUPPORT_ROWS`], then [`STRUCTURAL_RULES`], then [`INVALID_RULES`].
/// The names built from the input are [`CONSTRUCT_FAMILIES`].
pub static CONSTRUCT_NAMES: [&str; NAME_COUNT] = {
    let mut out = [""; NAME_COUNT];
    let mut at = 0;
    while at < ROW_COUNT {
        out[at] = SUPPORT_ROWS[at].name;
        at += 1;
    }
    let mut index = 0;
    while index < STRUCTURAL_RULES.len() {
        out[at] = STRUCTURAL_RULES[index].name;
        at += 1;
        index += 1;
    }
    index = 0;
    while index < INVALID_RULES.len() {
        out[at] = INVALID_RULES[index].name;
        at += 1;
        index += 1;
    }
    out
};
