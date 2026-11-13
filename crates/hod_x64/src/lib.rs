//! x86-64 code generation target.
//!
//! See `docs/ARCHITECTURE.md` for this crate's role and `spec/` for its
//! contract: `spec/codegen.md` §5.6 and §8.2 for the target,
//! `spec/abi-x64.md` (WP-118) for argument passing. The plan this crate
//! follows is `docs/design/0006-x86-64.md`.
//!
//! [`X64`] implements `hod_cg::Target`; [`compile_module`] and
//! [`compile_to_object`] take a module and its bodies to an object.
//!
//! # Modules and owners
//!
//! One module per instruction group, so that the group packages of
//! `docs/BACKLOG.md` work in parallel without touching the same file.
//! The dispatch in `target.rs` is final; a package fills in the entry
//! points of its module (each refuses as `x64:todo:<group>` until then),
//! adds the shared helpers it needs to `common` (each in its own
//! function, with a doc comment), and adds its test file under `tests/`.
//!
//! | module | owner | contents | tests |
//! |---|---|---|---|
//! | `reg` | WP-116, WP-140 | the registers as `hod_cg::PReg` indexes (x87 `st0`, `st1` too), DWARF numbers | unit, `tests/abi` |
//! | `abi` | WP-118, WP-140 | `assign`: where arguments and return values travel, on `hod_cg::abi`'s walk | `tests/abi` |
//! | `target` | WP-116, WP-140 | `X64`: the function's features, register description, classes, glue, the dispatch | `tests/skeleton.rs`, `tests/abi_followups.rs` |
//! | `frame` | WP-116 | prologue, exit sequences, CFI rows, frame addressing, dynamic slots, the call instruction | `tests/skeleton.rs` |
//! | `common` | WP-116 (shared) | moves, frame places, constants, symbol addresses, branches | `tests/skeleton.rs` |
//! | `term` | WP-116 | `br`, `ret`, `unreachable` | `tests/skeleton.rs` |
//! | `features` | WP-116, WP-117, WP-140 | the judged target features (§5.6.2), the function's (§5.6.9) | unit, `tests/abi_followups.rs` |
//! | `int` | WP-122 | integer binary operations, casts, `icmp`, `select`, `freeze` | `tests/int.rs` |
//! | `mem` | WP-123 | `load`, `store`, `alloca`, `ptradd`, `llvm.stacksave`/`stackrestore` | `tests/mem.rs` |
//! | `control` | WP-124 | `condbr`, `switch`, `indirectbr` | `tests/control.rs` |
//! | `call` | WP-125 | calls, `invoke`, `musttail`, landing pads, `resume`, variadics | `tests/call.rs` |
//! | `vcall` | WP-125, WP-132 | vectors across calls (`PartKind::Lanes`) | |
//! | `aggr` | WP-126 | `Mem`-class values: wide integers, aggregates, overflow intrinsics | `tests/aggr.rs` |
//! | `float` | WP-127 | scalar and vector-register floating point (SSE2) | `tests/float.rs` |
//! | `fpintr` | WP-127, WP-128 | the floating-point intrinsics | `tests/fpintr.rs` |
//! | `intrinsic` | WP-128 | the other intrinsics | `tests/intrinsic.rs` |
//! | `barrier` | WP-128 | inline assembly whose template assembles to nothing | `tests/barrier.rs` |
//! | `x87` | WP-131 | `x86_fp80` arithmetic, compares and conversions | |
//! | `vector` | WP-132 | `extractelement`, `insertelement`, `shufflevector` | |
//! | `lanes` | WP-132 | element-wise operations on `Mem`-class vectors | |
//! | `mask` | WP-132 | vectors of `i1` | |
//! | `atomic` | WP-133 | atomic `load`/`store`, `atomicrmw`, `cmpxchg`, `fence` | `tests/atomic.rs` |
//! | `tls` | WP-134 | thread-local addresses, `llvm.threadlocal.address`, `llvm.thread.pointer` | `tests/tls.rs` |
//! | `constaddr` | WP-126 | constant operands that hold addresses: differences, relocated images | `tests/aggr.rs` |
//! | `tail` | (later) | what no group package takes | |
//! | `coro` | WP-137 | coroutine handle operations: `llvm.coro.resume`, `destroy`, `done`, `promise`, `noop` | |

pub mod abi;
pub mod aggr;
pub mod atomic;
pub mod barrier;
pub mod call;
pub mod common;
pub mod constaddr;
pub mod control;
pub mod coro;
pub mod features;
pub mod float;
pub mod fpintr;
pub mod frame;
pub mod int;
pub mod intrinsic;
pub mod lanes;
pub mod mask;
pub mod mem;
pub mod reg;
pub mod tail;
pub mod target;
pub mod term;
pub mod tls;
pub mod vcall;
pub mod vector;
pub mod x87;

use std::fmt;

use hod_cg::{CgError, Chunk, Emitted, Options, Scratch, compile_function, emit_module};
use hod_ir::{Body, Module, SymId};

pub use target::{FnState, REG_INFO, X64};

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
/// order, for x86-64 and emits the module (`spec/codegen.md` §9.7): one
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
        compile_function(&X64, module, body, options, &mut scratch, &mut chunk)
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
