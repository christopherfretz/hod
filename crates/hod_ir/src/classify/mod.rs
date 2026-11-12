//! `classify` (`spec/ir.md` §8): the one place that decides whether hod
//! compiles a construct.
//!
//! - `matrix`: the support matrix, as data. Flipping a row is an edit
//!   there and a test.
//! - `support`: [`Support`], the matrix as a value a walk consults.
//! - `walk`: the module pass and the body pass, in the order of §8.8.
//! - `report`: the census and the text of `docs/status/support.md`.
//! - `attrs`: the attributes a call passes with, which the call sequence
//!   of `hod_cg` reads through the same functions.
//!
//! The accept path does not allocate and does not format: a walk reads
//! the tables once and consults an array of bit sets.

mod attrs;
mod matrix;
mod report;
mod support;
mod walk;

use std::borrow::Cow;
use std::fmt;

pub use attrs::{AttrConflict, call_arg_attrs, call_declaration, call_ret_attrs, merge_call_attrs};
pub use matrix::{
    CONSTRUCT_FAMILIES, CONSTRUCT_NAMES, ConstructInfo, INVALID_RULES, STRUCTURAL_RULES,
    SUPPORT_GROUPS, SUPPORT_ROWS, SupportCell, SupportRow, SupportStatus, TypeClass, TypeClasses,
};
pub use report::{Census, CensusEntry, support_markdown};
pub use support::Support;

use crate::body::{Body, CallSite};
use crate::ids::SymId;
use crate::module::Module;
use crate::symbol::Symbol;
use crate::value::InstId;
use walk::{First, Walker};

/// The two ways `classify` refuses its input (§8.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RejectionKind {
    /// A construct hod does not compile: a matrix row that is not
    /// supported, or a structural rule. An answer, not a failure: a
    /// driver reports the construct and may give the input to another
    /// backend.
    Unsupported,
    /// The module or body is not well formed (§8.7): its producer has a
    /// bug. A driver reports it as malformed input, not as a construct
    /// hod lacks.
    Invalid,
}

/// Why `classify` refused a module or a body: the first reason in the
/// order of §8.8.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Rejection {
    /// Unsupported, or malformed input.
    pub kind: RejectionKind,
    /// The construct's stable name, `group:name` (§8.5).
    pub construct: Cow<'static, str>,
    /// The symbol it was met at: the symbol itself in the module pass,
    /// the function in the body pass. `None` for a property of the
    /// module.
    pub symbol: Option<SymId>,
    /// In the body pass, the instruction it was met at; `None` for the
    /// function's signature and attributes.
    pub inst: Option<InstId>,
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            RejectionKind::Unsupported => write!(f, "unsupported construct {}", self.construct)?,
            RejectionKind::Invalid => write!(f, "malformed input: {}", self.construct)?,
        }
        if let Some(symbol) = self.symbol {
            write!(f, " at {symbol:?}")?;
        }
        if let Some(inst) = self.inst {
            write!(f, ", instruction {inst:?}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Rejection {}

/// The size of the stack-argument area of a signature, for the
/// `musttail` rules (§8.6).
///
/// Only a target's argument assignment knows it, and that lives above
/// this crate, so a caller that has one hands it in
/// ([`Classifier::with_stack_args`]). Without it, or when it answers
/// `None`, every `musttail` call is refused as
/// `musttail:stack-args-unknown`.
pub trait StackArgs {
    /// The bytes of stack the arguments of `function` occupy when it is
    /// called; `None` if the signature cannot be assigned.
    fn of_function(&self, module: &Module, function: Symbol<'_>) -> Option<u64>;

    /// The bytes of stack the arguments of the call `call` occupy;
    /// `None` if they cannot be assigned.
    fn of_call(&self, module: &Module, call: &CallSite<'_>) -> Option<u64>;
}

/// `classify` with a chosen [`Support`] and, optionally, the hook the
/// `musttail` rules need.
///
/// ```
/// use hod_ir::{Arch, Classifier, Module, Support, TargetSpec};
/// let module = Module::new(TargetSpec::aarch64_linux_gnu());
/// let everything = Support::everything_implemented(Arch::Aarch64);
/// assert!(Classifier::new(&everything).module(&module).is_ok());
/// ```
#[derive(Clone, Copy)]
pub struct Classifier<'a> {
    support: &'a Support,
    stack_args: Option<&'a dyn StackArgs>,
}

impl<'a> Classifier<'a> {
    /// A classifier that accepts what `support` supports.
    pub const fn new(support: &'a Support) -> Classifier<'a> {
        Classifier { support, stack_args: None }
    }

    /// The same, with the sizes of stack-argument areas from `hook`.
    #[must_use]
    pub const fn with_stack_args(mut self, hook: &'a dyn StackArgs) -> Classifier<'a> {
        self.stack_args = Some(hook);
        self
    }

    /// The matrix in use.
    pub const fn support(&self) -> &'a Support {
        self.support
    }

    /// The module pass (§8.8): the module's properties, flags and
    /// tables, then every symbol.
    ///
    /// # Errors
    /// The first reason the module is not compiled.
    pub fn module(&self, module: &Module) -> Result<(), Rejection> {
        let mut first = First::default();
        let _ = self.walker(module, &mut first).module();
        first.0.map_or(Ok(()), Err)
    }

    /// The body pass (§8.8): `body`'s function, then its instructions.
    ///
    /// # Errors
    /// The first reason the function is not compiled.
    pub fn body(&self, module: &Module, body: &Body) -> Result<(), Rejection> {
        let mut first = First::default();
        let _ = self.walker(module, &mut first).body(body);
        first.0.map_or(Ok(()), Err)
    }

    /// Adds every finding of the module pass to `census`.
    pub fn census_module(&self, module: &Module, census: &mut Census) {
        let _ = self.walker(module, census).module();
    }

    /// Adds every finding of the body pass over `body` to `census`.
    pub fn census_body(&self, module: &Module, body: &Body, census: &mut Census) {
        let _ = self.walker(module, census).body(body);
    }

    fn walker<'w, S>(&self, module: &'w Module, sink: &'w mut S) -> Walker<'w, S>
    where
        'a: 'w,
    {
        Walker { module, support: self.support, stack_args: self.stack_args, sink }
    }
}

impl fmt::Debug for Classifier<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Classifier")
            .field("support", self.support)
            .field("stack_args", &self.stack_args.is_some())
            .finish()
    }
}

/// Decides whether hod compiles `module`'s module-level contents: its
/// properties, flags, tables and symbols (§8.1). The bodies are
/// classified one by one with [`classify_body`].
///
/// Uses [`Support::shipped`] for the module's target and no
/// [`StackArgs`]; [`Classifier`] is the general form.
///
/// # Errors
/// The first reason, in the order of §8.8.
pub fn classify(module: &Module) -> Result<(), Rejection> {
    Classifier::new(Support::shipped(module.target().arch())).module(module)
}

/// Decides whether hod compiles the function `body` of `module` (§8.1).
///
/// # Errors
/// The first reason, in the order of §8.8.
pub fn classify_body(module: &Module, body: &Body) -> Result<(), Rejection> {
    Classifier::new(Support::shipped(module.target().arch())).body(module, body)
}

/// Counts every reason `module` and `bodies` are not compiled under
/// [`Support::shipped`] (§8.9).
pub fn census<'b>(module: &Module, bodies: impl IntoIterator<Item = &'b Body>) -> Census {
    let classifier = Classifier::new(Support::shipped(module.target().arch()));
    let mut census = Census::new();
    classifier.census_module(module, &mut census);
    for body in bodies {
        classifier.census_body(module, body, &mut census);
    }
    census
}
