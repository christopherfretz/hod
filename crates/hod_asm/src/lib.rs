//! Assembler-syntax parser for module-level and inline assembly
//! (`spec/asm.md`).
//!
//! hod writes no assembly text and runs no assembler: what this crate
//! accepts it parses and encodes itself, through `hod_mc_a64`'s checked
//! encoders, and what it does not understand it refuses by a stable
//! name ([`Refusal`]). A wrong guess is the one real failure, so every
//! form accepted here is one whose bytes were compared with LLVM's
//! assembler (`tests/llvm_mc.rs`).
//!
//! - [`text`]: statements of assembly text: separators, comments.
//! - [`expr`]: integer expressions and symbol references.
//! - [`template`]: the operand substitution of an inline-assembly
//!   template (`$0`, `${0:w}`, `$$`, `${:uid}`, `$(..$|..$)`), as LLVM's
//!   `AsmPrinter` does it before its assembler reads the text.
//! - [`a64`]: AArch64 instruction statements to instruction words, and
//!   the statement loop shared by inline and module-level assembly.
//!
//! Nothing here allocates per instruction beyond the output it returns.

#![forbid(unsafe_code)]

pub mod a64;
pub mod expr;
pub mod reason;
pub mod template;
pub mod text;

pub use reason::Refusal;
