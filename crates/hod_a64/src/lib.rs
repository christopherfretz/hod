//! AArch64 code generation target.
//!
//! See `docs/ARCHITECTURE.md` for this crate's role and `spec/` for its
//! contract: `spec/codegen.md` §5.5 and §8.2 for the target,
//! `spec/abi-a64.md` for argument passing.
//!
//! [`A64`] implements `hod_cg::Target`; [`compile_module`] and
//! [`compile_to_object`] take a module and its bodies to an object.
//!
//! # Modules and owners
//!
//! One module per instruction group, so that the group packages of
//! `docs/BACKLOG.md` work in parallel without touching the same file.
//! The dispatch in `target.rs` is final; a package fills in the entry
//! points of its module, adds the shared helpers it needs to `common`
//! (each in its own function, with a doc comment), adds its test file
//! under `tests/`, and enables its constructs in the differential
//! harness by one line of `tests/common/groups.rs`.
//!
//! | module | owner | contents | tests |
//! |---|---|---|---|
//! | `reg` | WP-016 | the registers as `hod_cg::PReg` indexes | unit |
//! | `abi` | WP-016 | `assign`: where arguments and return values travel | `tests/abi` |
//! | `target` | WP-033 | `A64`: register description, classes, glue, the dispatch | `tests/skeleton.rs` |
//! | `frame` | WP-033 | prologue, exit sequences, CFI rows, incoming base, dynamic slots, the call instruction | `tests/frame.rs` |
//! | `common` | WP-033 (shared) | addressing, moves, constants, symbol addresses, branches | `tests/skeleton.rs` |
//! | `term` | WP-033 | `br`, `ret`, `unreachable` | `tests/skeleton.rs`, `tests/native.rs` |
//! | `int` | WP-035 | integer binary operations, casts, `icmp`, `select`, `freeze` | `tests/int.rs` |
//! | `mem` | WP-036 | `load`, `store`, `alloca`, `ptradd`, `llvm.stacksave`/`stackrestore` | `tests/mem.rs` |
//! | `control` | WP-037 | `condbr`, `switch`, `indirectbr`, `blockaddress` | `tests/control.rs` |
//! | `call` | WP-038 | calls, `invoke`, `musttail`, landing pads, `resume`, variadics | `tests/call.rs` |
//! | `float` | WP-039 | floating-point and vector-register instructions | `tests/float.rs` |
//! | `aggr` | WP-040 | `Mem`-class values: wide integers, aggregates, overflow intrinsics | `tests/aggr.rs` |
//! | `intrinsic` | WP-041, WP-070 | the other intrinsics | `tests/intrinsic.rs` |
//! | `fpintr` | WP-070 | the floating-point intrinsics | `tests/fpintr.rs` |
//!
//! The differential harness that runs generated programs natively and
//! compares them with `hod_interp` is `tests/diff.rs`.

pub mod abi;
pub mod aggr;
pub mod call;
pub mod common;
pub mod control;
pub mod float;
pub mod fpintr;
pub mod frame;
pub mod int;
pub mod intrinsic;
pub mod mem;
pub mod reg;
pub mod target;
pub mod term;

use std::fmt;

use hod_cg::{CgError, Chunk, Emitted, Options, Scratch, compile_function, emit_module};
use hod_ir::{Body, Module, SymId};

pub use target::{A64, FnState, REG_INFO};

/// Why a module could not be turned into an object.
#[derive(Debug)]
pub enum ModuleError {
    /// A function could not be compiled.
    Function {
        /// The function.
        sym: SymId,
        /// Why.
        error: CgError,
    },
    /// Module emission failed.
    Emit(CgError),
    /// The object could not be written.
    Object(hod_obj::Error),
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleError::Function { sym, error } => write!(f, "function {sym:?}: {error}"),
            ModuleError::Emit(error) => write!(f, "module emission: {error}"),
            ModuleError::Object(error) => write!(f, "object: {error}"),
        }
    }
}

impl std::error::Error for ModuleError {}

/// Compiles `bodies`, the bodies of `module`'s defined functions in module
/// order, for AArch64 and emits the module (`spec/codegen.md` §9.7): one
/// worker, one chunk.
///
/// # Errors
/// The first function that does not compile, or what module emission
/// reports.
pub fn compile_module(
    module: &Module,
    bodies: &[Body],
    options: &Options,
) -> Result<Emitted, ModuleError> {
    let mut scratch = Scratch::new();
    let mut chunk = Chunk::new();
    for body in bodies {
        compile_function(&A64, module, body, options, &mut scratch, &mut chunk)
            .map_err(|error| ModuleError::Function { sym: body.symbol(), error })?;
    }
    emit_module(module, options, std::slice::from_ref(&chunk)).map_err(ModuleError::Emit)
}

/// [`compile_module`], then the ELF relocatable object's bytes.
///
/// # Errors
/// As [`compile_module`], and what writing the object reports.
pub fn compile_to_object(
    module: &Module,
    bodies: &[Body],
    options: &Options,
) -> Result<Vec<u8>, ModuleError> {
    let emitted = compile_module(module, bodies, options)?;
    hod_obj::elf::to_vec(&emitted.object).map_err(ModuleError::Object)
}
