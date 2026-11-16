//! `hod_x86`: code generation for 32-bit x86 (i686 Linux, the i386
//! System V psABI), design `docs/design/0008-i386.md`,
//! `spec/codegen-x86.md`. It implements `hod_cg::Target` ([`X86`]) over
//! `hod_mc_x64`'s encoder in its 32-bit mode (`Mode::X86`, through which
//! every instruction passes: `common::put`).
//!
//! WP-293 (milestone A) lowers the integers of class `Gpr` (`i1`..`i32`,
//! `ptr`), memory, control flow and direct and indirect calls with stack
//! arguments, in the static relocation model. Everything else is refused
//! under a stable `x86:` name, one per construct family; the module
//! table in `README.md` says which package removes each.
#![forbid(unsafe_code)]

pub mod abi;
pub mod alu;
pub mod asm;
pub mod bits;
pub mod bytes;
pub mod call;
pub mod common;
pub mod control;
pub mod frame;
pub mod int;
pub mod mem;
pub mod pic;
pub mod reg;
pub mod target;
pub mod tls;
pub mod va;
pub mod x87;

use std::fmt;

use hod_cg::{CgError, Chunk, Emitted, Options, Scratch, compile_function, emit_module};
use hod_ir::{Body, Module, SymId};

pub use target::{FnState, REG_INFO, X86};

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
/// order, for i686 and emits the module: one worker, one chunk.
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
        compile_function(&X86, module, body, options, &mut scratch, &mut chunk)
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
