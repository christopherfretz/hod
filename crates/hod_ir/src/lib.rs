//! The IR hod consumes: types, constants, globals, functions, instructions, debug metadata.
//!
//! See `docs/ARCHITECTURE.md` for this crate's role and `spec/ir.md` for its
//! contract. This file only declares the modules and re-exports their
//! public items; each table lives in its own module:
//!
//! | module | `spec/ir.md` | contents |
//! |---|---|---|
//! | `ids` | §1 | the `u32` id newtypes |
//! | `error` | §1 | `IrError`, `IrErrorKind` |
//! | `target` | §2.1 | `TargetSpec`, `DataLayout` |
//! | `types` | §3 | `TypeTable` and layout |
//! | `constant` | §4.1, §4.2 | `ConstPool` |
//! | `fold` | §4.2 | constant-expression folding |
//! | `flatten` | §4.3 | initializers as bytes and fixups |
//! | `attrs` | §5.7 | `CallConv`, `ParamAttrs`, `FnAttrs`, the attribute table |
//! | `module` | §2 | `Module` and its builder API |
//! | `symbol` | §2.2 | the symbol table |
//! | `comdat` | §2 | the COMDAT table |
//! | `asm` | §2 | the inline-assembly table |
//! | `props` | §2.1 | `ModuleProps` and module flags |
//! | `strings` | §2 | interned byte strings |
//! | `value` | §5.3 | `Value`, `InstId`, `SlotId`, `MetaId` |
//! | `inst` | §5.4 | opcodes, predicates, memory flags, the instruction record |
//! | `intrinsic` | §5.5 | `Intrinsic`, name resolution, the no-code rule |
//! | `body` | §5.1, §5.2, §5.6 | `Body` and the API code generation reads |
//! | `builder` | §5.1, §5.3 | `FunctionBuilder` |
//! | `verify` | §5.1 | `Body::verify` |
//! | `debug` | §6 | `DebugInfo`: debug nodes, expressions, locations, the field table |
//! | `classify` | §8 | the support matrix, `classify`, the census |
//! | `print` | §9 | the printer: LLVM text from a module and its bodies |

mod asm;
mod asm_text;
mod attrs;
mod body;
mod builder;
mod classify;
mod comdat;
mod constant;
mod dom;
mod error;
mod flatten;
mod fold;
mod ids;
mod inst;
/// The storage of a module's tables (`spec/ir.md` §5.1): public for the
/// [`Layering`] parameter only.
#[doc(hidden)]
pub mod intern;
mod intrinsic;
mod module;
mod parallel;
mod print;
mod props;
#[cfg(test)]
mod random;
mod strings;
mod symbol;
mod target;
mod types;
mod value;
mod verify;
mod wide;

mod debug;

pub use asm::InlineAsm;
pub use asm_text::{
    AsmBinding, AsmDirectives, AsmRefusal, AsmSymbolEffect, AsmSymbolType, asm_template_is_empty,
    module_asm_effects, read_module_asm,
};
pub use attrs::{
    ATTRIBUTES, AttrClass, AttrInfo, AttrSyntax, FnAttrs, FnAttrsId, FnAttrsView, FnStr, UwTable,
    attr_class, attr_info,
};
pub use attrs::{CallConv, ParamAttrs, ParamMem};
pub use comdat::{Comdat, ComdatKind};
pub use constant::{Const, ConstOp, ConstPool};
pub use error::{IrError, IrErrorKind, IrResult};
pub use flatten::{BlockDiff, Fixup, FixupTarget, NegAddr};
pub use ids::{AsmId, BlockId, ComdatId, ConstId, DiId, LocId, SymId, TypeId};
pub use intern::{Layered, Layering, NoLayer};
pub use module::{Delta, Mark, Module, Remap, Stamp, StringLists, Structor, Unsupported};
pub use parallel::{BodyRead, BodySource, CHUNK_WEIGHT, ReadStats, read_bodies};
pub use print::{Dialect, IoSink, Part, PrintError};
pub use props::{
    BranchProtection, CodeModel, FlagBehavior, FlagClass, FlagValue, FramePointer,
    IGNORABLE_MODULE_FLAGS, KNOWN_MODULE_FLAGS, ModuleFlag, ModuleProps, PicLevel,
    module_flag_class,
};
pub use symbol::{
    AliasDesc, FunctionDesc, IFuncDesc, Linkage, Retained, Symbol, SymbolKind, TlsModel,
    UnnamedAddr, VariableDesc, Visibility,
};
pub use target::{
    AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19, AlignPair, Arch, DataLayout,
    FunctionPtrAlignKind, MAX_ADDR_SPACE, Mangling, Os, PointerSpec, PrimSpec, TargetSpec,
    X86_64_LINUX_DATA_LAYOUT,
};
pub use types::{MAX_INT_BITS, MAX_TYPE_SIZE, TypeKind, TypeTable};

pub use body::{
    Block, Blocks, Body, Bundle, CallSite, Callee, Clause, Inst, InstKind, Insts, LandingPad,
    LifetimeMarker, Metadata, Phi, PtrAdd, SHUFFLE_UNDEF, Slot, SlotVar, Successors, Switch,
    UseFacts,
};
pub use builder::{CallDesc, FunctionBuilder};
pub use inst::{
    AtomicOrdering, FloatPredicate, IntPredicate, MemFlags, Opcode, RmwOp, SyncScope, TailKind,
    aggregate_field,
};
pub use intrinsic::{
    Intrinsic, IntrinsicRef, NoCode, lifetime_intrinsic, no_code_intrinsic, no_code_names,
    resolve_intrinsic,
};
pub use value::{InstId, MetaId, SlotId, Value, ValueKind};
pub use verify::cast_is_valid;
pub use wide::sext64;

pub use debug::expr::{DI_OPS, DiOp, DiOps, ExprId, di_op_named};
pub use debug::map::{
    DI_FIELD_NOTES, DI_KIND_NOTES, DI_RECORDS, DiMapping, di_field, di_field_table, di_kind,
    di_record,
};
// The `DW_TAG_*` numbers, the name tables and lookups, the flag sets.
pub use body::{DbgDeclare, DbgRecord, DbgRecordKind};
pub use classify::{
    AttrConflict, CONSTRUCT_FAMILIES, CONSTRUCT_NAMES, Census, CensusEntry, Classifier,
    ConstructInfo, INVALID_RULES, Rejection, RejectionKind, STRUCTURAL_RULES, SUPPORT_GROUPS,
    SUPPORT_ROWS, StackArgs, Support, SupportCell, SupportRow, SupportStatus, TypeClass,
    TypeClasses, call_arg_attrs, call_declaration, call_ret_attrs, census, classify, classify_body,
    merge_call_attrs, support_markdown,
};
pub use debug::names::*;
pub use debug::node::{
    DiBasicType, DiBound, DiCompileUnit, DiCompositeType, DiConstant, DiDerivedType, DiEnumerator,
    DiFieldOut, DiFieldValue, DiFile, DiGlobalVariable, DiGlobalVariableExpression,
    DiImportedEntity, DiKind, DiLabel, DiLexicalBlock, DiLexicalBlockFile, DiLocalVariable,
    DiNamespace, DiNode, DiOther, DiShape, DiString, DiSubprogram, DiSubrange, DiSubroutineType,
    DiTemplateTypeParameter, DiTemplateValueParameter, DiTuple,
};
pub use debug::{DebugInfo, DebugStats, DiLocation};
