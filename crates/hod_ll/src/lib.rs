//! LLVM textual IR parser producing `hod_ir`.
//!
//! The contract is `spec/ll.md`. This crate has the lexer ([`Lexer`]) and
//! the module-level parser ([`parse_module`]): everything of an `.ll`
//! file except the bodies of its functions, which are found, stepped
//! over, and handed on as byte spans ([`ModuleText::functions`]) with
//! the tables a body parser needs; and the body parser ([`BodyParser`],
//! [`parse_bodies`], `spec/ll.md` §11), which reads those spans into
//! `hod_ir` bodies, one at a time or in parallel.

mod attrs;
mod body;
mod call;
mod consts;
mod debug;
mod error;
mod float;
mod globals;
mod index;
mod inst;
mod lexer;
mod meta;
mod names;
mod parser;
mod skip;
mod types;

use hod_ir::{FnAttrsId, Module, SymId, TargetSpec, TypeId};

pub use attrs::call_conv_keyword;
pub use body::{BodyParser, HAND_OVER_BYTES, ParsedBody, parse_bodies, parse_bodies_on};
pub use error::{LlError, LlErrorKind};
pub use lexer::{Lexer, MAX_SOURCE_BYTES, Tok, Token, unescape};
pub use meta::{MdKind, MdNode, MdRef, MetadataIndex, NamedMetadata};

/// The limits a source may not pass (`spec/ll.md` §8); passing one is an
/// [`LlErrorKind::Limit`] error.
pub mod limits {
    pub use crate::lexer::MAX_SOURCE_BYTES;

    /// How deeply types and constants may be nested, counting each
    /// aggregate, each constant expression and each reference to a named
    /// type under definition.
    pub const MAX_NESTING: u32 = 200;

    /// The most decimal digits an integer literal may have.
    pub const MAX_INT_LITERAL_DIGITS: usize = 1 << 16;
}

/// Bytes `start..end` of the source.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Span {
    /// Offset of the first byte.
    pub start: u32,
    /// Offset one past the last byte.
    pub end: u32,
}

impl Span {
    /// The bytes of `source` the span covers.
    ///
    /// # Panics
    /// If the span is not inside `source`.
    pub fn of<'a>(&self, source: &'a [u8]) -> &'a [u8] {
        &source[self.start as usize..self.end as usize]
    }
}

/// What [`parse_module`] needs besides the text.
#[derive(Clone, Debug, Default)]
pub struct ParseOptions {
    /// The target of a module that has no `target triple`. Without one,
    /// such a module is an [`LlErrorKind::Target`] error.
    pub default_target: Option<TargetSpec>,
}

/// One function definition: its symbol and where its text is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FunctionText {
    /// The function's symbol.
    pub sym: SymId,
    /// The header: from after `define` up to the `{` of the body.
    pub header: Span,
    /// The parameter list, between its parentheses.
    pub params: Span,
    /// The body, between its braces.
    pub body: Span,
    /// The `!dbg` attachment of the definition (its `DISubprogram`).
    pub dbg: Option<MdRef>,
}

/// One attribute group, `attributes #id = { … }`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AttrGroup {
    /// Its number.
    pub id: u32,
    /// The kept function attributes of the group (`spec/ir.md` §5.7),
    /// interned in the module.
    pub attrs: FnAttrsId,
    /// The `align` of the group.
    pub align: Option<u64>,
    /// The names of its rejected attributes: whoever applies the group
    /// to a function or a call records each as `attr:<name>`.
    pub rejected: Vec<Box<str>>,
}

/// What the text of a module leaves behind for whoever parses the
/// function bodies (`spec/ll.md` §6): where each body is, and the tables
/// that resolve what a body refers to by name or number. Read-only and
/// `Sync`: any number of body parsers may share one.
#[derive(Debug)]
pub struct ModuleText<'a> {
    source: &'a [u8],
    functions: Vec<FunctionText>,
    named_types: names::NameMap,
    numbered_types: Vec<(u32, u32)>,
    numbered_globals: Vec<(u32, SymId)>,
    groups: Vec<AttrGroup>,
    metadata: MetadataIndex,
    debug_map: debug::DebugMap,
}

impl<'a> ModuleText<'a> {
    /// The source the spans are offsets into.
    pub fn source(&self) -> &'a [u8] {
        self.source
    }

    /// The function definitions, in file order.
    pub fn functions(&self) -> &[FunctionText] {
        &self.functions
    }

    /// The type `%name` names. `name` is the name's bytes, unescaped,
    /// without the `%` or quotes.
    pub fn named_type(&self, name: &[u8]) -> Option<TypeId> {
        self.named_types.get(name).map(TypeId::new)
    }

    /// The type `%number` names.
    pub fn numbered_type(&self, number: u32) -> Option<TypeId> {
        let at = self.numbered_types.binary_search_by_key(&number, |&(n, _)| n).ok()?;
        Some(TypeId::new(self.numbered_types[at].1))
    }

    /// The symbol `@number` names.
    pub fn numbered_global(&self, number: u32) -> Option<SymId> {
        let at = self.numbered_globals.binary_search_by_key(&number, |&(n, _)| n).ok()?;
        Some(self.numbered_globals[at].1)
    }

    /// The attribute group `#id`. A group that is used but never defined
    /// is empty, as in LLVM, and is not listed.
    pub fn attr_group(&self, id: u32) -> Option<&AttrGroup> {
        let at = self.groups.binary_search_by_key(&id, |group| group.id).ok()?;
        Some(&self.groups[at])
    }

    /// Every attribute group, by ascending id.
    pub fn attr_groups(&self) -> &[AttrGroup] {
        &self.groups
    }

    /// The metadata index.
    pub fn metadata(&self) -> &MetadataIndex {
        &self.metadata
    }

    /// What each numbered metadata node became, by position in the
    /// metadata index.
    pub(crate) fn debug_map(&self) -> &debug::DebugMap {
        &self.debug_map
    }
}

/// The result of [`parse_module`]: the module with everything but
/// function bodies, and what a body parser needs to finish the job
/// (`spec/ll.md` §6). The two are separate fields so that the module can
/// be added to while the text tables are read; the tables are also
/// reached through the result itself (`parsed.functions()`).
#[derive(Debug)]
pub struct ParsedModule<'a> {
    /// The module: target, types, constants, symbols, COMDATs, attribute
    /// sets, module properties, and the record of what was unsupported.
    pub module: Module,
    /// The text side.
    pub text: ModuleText<'a>,
}

impl<'a> std::ops::Deref for ParsedModule<'a> {
    type Target = ModuleText<'a>;

    fn deref(&self) -> &ModuleText<'a> {
        &self.text
    }
}

/// Parses the module level of the LLVM textual IR in `source`
/// (`spec/ll.md` §4).
///
/// # Errors
/// [`LlError`] with the position of the first thing that is not LLVM IR
/// or that `hod_ir` refuses. An unsupported construct is not an error:
/// it is in `module.unsupported()`.
pub fn parse_module<'a>(
    source: &'a [u8],
    options: &ParseOptions,
) -> Result<ParsedModule<'a>, LlError> {
    parser::run(source, options).map_err(|failure| failure.locate(source))
}

/// The number of top-level entities of `source` and the bytes of
/// function bodies among them: the index scan alone (`spec/ll.md`
/// §4.1), for measurements.
///
/// # Errors
/// As [`parse_module`], for what the scan can see.
pub fn scan_only(source: &[u8]) -> Result<(usize, usize), LlError> {
    let index = index::scan(source).map_err(|failure| failure.locate(source))?;
    let entities = index.items.len()
        + index.types.len()
        + index.comdats.len()
        + index.groups.len()
        + index.nodes.len()
        + index.named.len();
    let bodies = index.items.iter().map(|item| (item.body.end - item.body.start) as usize).sum();
    Ok((entities, bodies))
}
