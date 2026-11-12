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
//! | `attrs` | §5.7 | `CallConv`, `ParamAttrs` |
//! | `module` | §2 | the `Module` shell |

mod attrs;
mod constant;
mod error;
mod flatten;
mod fold;
mod ids;
mod intern;
mod module;
mod target;
mod types;
mod wide;

pub use attrs::{CallConv, ParamAttrs, ParamMem};
pub use constant::{Const, ConstOp, ConstPool};
pub use error::{IrError, IrErrorKind, IrResult};
pub use flatten::{Fixup, FixupTarget};
pub use ids::{AsmId, BlockId, ComdatId, ConstId, DiId, LocId, SymId, TypeId};
pub use module::Module;
pub use target::{
    AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19, AlignPair, Arch, DataLayout,
    FunctionPtrAlignKind, MAX_ADDR_SPACE, Mangling, Os, PointerSpec, PrimSpec, TargetSpec,
    X86_64_LINUX_DATA_LAYOUT,
};
pub use types::{MAX_INT_BITS, MAX_TYPE_SIZE, TypeKind, TypeTable};
