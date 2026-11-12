//! Debug information (`spec/bc.md` §6.3): the specialized metadata
//! records of the module's METADATA block and of each function's, read
//! into `hod_ir`'s debug tables with the fields, defaults and refusals
//! `hod_ll` gives the same nodes as text (`spec/ll.md` §7.4).
//!
//! As in `hod_ll`, two passes over a block's ids and no recursion
//! through references: the first hands every node an id (a reserved
//! debug node, a reserved location, an interned expression), the second
//! defines each node from its record with every reference resolved. A
//! string, a constant and a tuple a field names as a single node get
//! their id when first named; a field that holds a list reads the
//! tuple's operands in place.
//!
//! Each record kind has one layout table, by operand position, for the
//! layouts LLVM 19 to 22 write; a record with more operands than LLVM 22
//! writes, or a version LLVM 19 does not write, is refused as
//! `debug-layout` rather than guessed at.

use hod_ir::{
    DiConstant, DiFieldValue, DiId, DiKind, DiLocation, DiMapping, DiNode, DiOther, DiString,
    DiTuple, ExprId, LocId, Module, di_field,
};

use crate::bitstream::{BitReader, BlockInfo, BlockSpan, Cursor, Entry};
use crate::codes::{block, md};
use crate::consts::Values;
use crate::error::{BcErrorKind, BcResult, fail, ir};
use crate::raw::{MdSlot, Raw};

/// The construct recorded when debug metadata breaks a rule LLVM's
/// verifier checks (`hod_ll`'s `debug:invalid`).
pub(crate) const INVALID: &str = "debug:invalid";

/// The refusal of a record layout this reader does not know: one LLVM 19
/// to 22 does not write (older, or newer than LLVM 22).
pub(crate) const LAYOUT: BcErrorKind = BcErrorKind::Unsupported("debug-layout");

/// What a metadata id became.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Handle {
    /// Nothing (yet): a string, constant or tuple no field has named,
    /// or metadata that is not debug information.
    None,
    Node(DiId),
    Loc(LocId),
    Expr(ExprId),
    /// A `DIAssignID`: nothing is kept (`spec/ir.md` §6.9).
    AssignId,
}

/// One function-local metadata id, as a body's records use it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum LocalMd {
    /// A debug node, location or expression.
    Debug(Handle),
    /// A value as metadata: its bitcode type id and absolute value id.
    Value {
        ty: u64,
        value: u64,
    },
    /// `!{}`.
    EmptyTuple,
    /// A `DIArgList`: its operands (metadata ids) in [`DebugBits::args`].
    ArgList {
        start: u32,
        len: u32,
    },
    Other,
}

/// A function's local metadata: `local[start..start + len]`, numbered
/// from the module's metadata count.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FnDebug {
    pub start: u32,
    pub len: u32,
}

/// What the body reader needs of the debug information.
#[derive(Debug, Default)]
pub(crate) struct DebugBits {
    /// Whether the module has debug information at all.
    pub on: bool,
    /// By module metadata id.
    pub module: Vec<Handle>,
    /// By the first bit of the FUNCTION block, sorted.
    pub functions: Vec<(u64, FnDebug)>,
    pub local: Vec<LocalMd>,
    pub args: Vec<u64>,
}

impl DebugBits {
    /// The local metadata of the function whose block starts at `start`.
    pub fn function(&self, start: u64) -> FnDebug {
        match self.functions.binary_search_by_key(&start, |&(at, _)| at) {
            Ok(found) => self.functions[found].1,
            Err(_) => FnDebug::default(),
        }
    }
}

/// One record of a function's METADATA block.
#[derive(Clone, Copy, Debug)]
pub(crate) enum LSlot<'a> {
    String(&'a [u8]),
    Rec { code: u32, pos: u64, start: u32, len: u32 },
}

/// A field's value as read, before the node is built.
#[derive(Clone, Copy, Debug)]
enum F<'a> {
    UInt(u64),
    Int(i64),
    Bool(bool),
    Str(&'a [u8]),
    Node(Option<DiId>),
    /// `lists[start..end]`.
    List(u32, u32),
    Expr(ExprId),
}

/// Scratch storage kept from one node to the next.
#[derive(Debug, Default)]
pub(crate) struct Scratch<'a> {
    tuples: Vec<(DiId, u64)>,
    lists: Vec<DiId>,
    fields: Vec<(&'static str, F<'a>)>,
}

/// A function's METADATA block, as read.
#[derive(Debug, Default)]
pub(crate) struct LocalBlock<'a> {
    pub slots: Vec<LSlot<'a>>,
    pub pool: Vec<u64>,
    pub handles: Vec<Handle>,
}

/// How an operand becomes a field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum How {
    /// Not a field (the flags operand), or one read by hand.
    Skip,
    /// A field `hod_ir` drops.
    Drop,
    UInt,
    Bool,
    /// A single node, metadata id + 1.
    Node,
    /// A tuple's operands, metadata id + 1.
    List,
    /// A string, metadata id + 1.
    Str,
    /// An expression, metadata id + 1.
    Expr,
    /// A subrange bound: a constant, a node or an expression.
    Bound,
    /// A size or offset: a number, or (LLVM 21 and later, with the
    /// record's size-is-metadata flag) a constant as metadata.
    Size,
    /// A number + 1, 0 for none (`dwarfAddressSpace`).
    PlusOne,
    /// A number, `u64::MAX` for none (`coroSuspendIdx`).
    Opt,
    /// An unsupported field, a number whose default is the value given.
    UnsupInt(u64),
    /// An unsupported field, metadata id + 1, whose default is null.
    UnsupMd,
    /// `DIDerivedType`'s packed pointer-authentication fields.
    PtrAuth,
}

/// The layout of one record kind: the kind's LLVM name, the `hod_ir`
/// kind (`None` for one it does not represent), how many operands LLVM
/// 19 to 22 write, and each operand by position.
struct Layout {
    name: &'static str,
    kind: Option<DiKind>,
    min: usize,
    max: usize,
    fields: &'static [(&'static str, How)],
}

use How::{
    Bool, Bound, Drop, Expr, List, Node, Opt, PlusOne, PtrAuth, Size, Skip, Str, UInt, UnsupInt,
    UnsupMd,
};

const ENUM_KIND_NONE: u64 = 0xFFFF_FFFF;

/// The layouts, by record code (`spec/bc.md` §6.3).
#[allow(clippy::too_many_lines)] // One table.
fn layout(code: u32) -> Option<Layout> {
    let l =
        |name, kind, min, max, fields| Some(Layout { name, kind: Some(kind), min, max, fields });
    let other = |name| Some(Layout { name, kind: None, min: 1, max: usize::MAX, fields: &[] });
    match code {
        md::SUBRANGE => l(
            "DISubrange",
            DiKind::Subrange,
            5,
            5,
            &[
                ("", Skip),
                ("count", Bound),
                ("lowerBound", Bound),
                ("upperBound", Bound),
                ("stride", Bound),
            ],
        ),
        // Read by hand: the value is a wide integer.
        md::ENUMERATOR => l("DIEnumerator", DiKind::Enumerator, 3, usize::MAX, &[]),
        md::BASIC_TYPE => l(
            "DIBasicType",
            DiKind::BasicType,
            6,
            9,
            &[
                ("", Skip),
                ("tag", UInt),
                ("name", Str),
                ("size", Size),
                ("align", UInt),
                ("encoding", UInt),
                ("flags", UInt),
                ("num_extra_inhabitants", UnsupInt(0)),
                ("dataSize", UInt),
            ],
        ),
        md::FILE => l(
            "DIFile",
            DiKind::File,
            3,
            6,
            &[
                ("", Skip),
                ("filename", Str),
                ("directory", Str),
                ("checksumkind", UInt),
                ("checksum", Str),
                ("source", Str),
            ],
        ),
        md::DERIVED_TYPE => l(
            "DIDerivedType",
            DiKind::DerivedType,
            12,
            15,
            &[
                ("", Skip),
                ("tag", UInt),
                ("name", Str),
                ("file", Node),
                ("line", UInt),
                ("scope", Node),
                ("baseType", Node),
                ("size", Size),
                ("align", UInt),
                ("offset", Size),
                ("flags", UInt),
                ("extraData", Node),
                ("dwarfAddressSpace", PlusOne),
                ("annotations", UnsupMd),
                ("", PtrAuth),
            ],
        ),
        md::COMPOSITE_TYPE => l(
            "DICompositeType",
            DiKind::CompositeType,
            16,
            26,
            &[
                ("", Skip),
                ("tag", UInt),
                ("name", Str),
                ("file", Node),
                ("line", UInt),
                ("scope", Node),
                ("baseType", Node),
                ("size", Size),
                ("align", UInt),
                ("offset", Size),
                ("flags", UInt),
                ("elements", List),
                ("runtimeLang", UInt),
                ("vtableHolder", Node),
                ("templateParams", List),
                ("identifier", Str),
                ("discriminator", UnsupMd),
                ("dataLocation", UnsupMd),
                ("associated", UnsupMd),
                ("allocated", UnsupMd),
                ("rank", UnsupMd),
                ("annotations", UnsupMd),
                ("num_extra_inhabitants", UnsupInt(0)),
                ("specification", UnsupMd),
                ("enumKind", UnsupInt(ENUM_KIND_NONE)),
                ("bitStride", UnsupMd),
            ],
        ),
        md::SUBROUTINE_TYPE => l(
            "DISubroutineType",
            DiKind::SubroutineType,
            3,
            4,
            &[("", Skip), ("flags", UInt), ("types", List), ("cc", UInt)],
        ),
        md::COMPILE_UNIT => l(
            "DICompileUnit",
            DiKind::CompileUnit,
            14,
            23,
            &[
                ("", Skip),
                // The language, read by hand.
                ("", Skip),
                ("file", Node),
                ("producer", Str),
                ("isOptimized", Bool),
                ("flags", Str),
                ("runtimeVersion", UInt),
                ("splitDebugFilename", UnsupMd),
                ("emissionKind", UInt),
                ("enums", List),
                ("retainedTypes", List),
                // The subprograms of a bitcode before LLVM 3.9: checked
                // to be none.
                ("", Skip),
                ("globals", List),
                ("imports", List),
                ("dwoId", UnsupInt(0)),
                ("macros", UnsupMd),
                ("splitDebugInlining", Drop),
                ("debugInfoForProfiling", Drop),
                ("nameTableKind", UInt),
                ("rangesBaseAddress", Drop),
                ("sysroot", Str),
                ("sdk", Str),
                ("sourceLanguageVersion", UnsupInt(0)),
            ],
        ),
        md::SUBPROGRAM => l(
            "DISubprogram",
            DiKind::Subprogram,
            18,
            21,
            &[
                ("", Skip),
                ("scope", Node),
                ("name", Str),
                ("linkageName", Str),
                ("file", Node),
                ("line", UInt),
                ("type", Node),
                ("scopeLine", UInt),
                ("containingType", Node),
                ("spFlags", UInt),
                ("virtualIndex", UInt),
                ("flags", UInt),
                ("unit", Node),
                ("templateParams", List),
                ("declaration", Node),
                ("retainedNodes", List),
                ("thisAdjustment", Drop),
                ("thrownTypes", List),
                ("annotations", UnsupMd),
                ("targetFuncName", UnsupMd),
                ("keyInstructions", Drop),
            ],
        ),
        md::LEXICAL_BLOCK => l(
            "DILexicalBlock",
            DiKind::LexicalBlock,
            5,
            5,
            &[("", Skip), ("scope", Node), ("file", Node), ("line", UInt), ("column", UInt)],
        ),
        md::LEXICAL_BLOCK_FILE => l(
            "DILexicalBlockFile",
            DiKind::LexicalBlockFile,
            4,
            4,
            &[("", Skip), ("scope", Node), ("file", Node), ("discriminator", UInt)],
        ),
        md::NAMESPACE => {
            l("DINamespace", DiKind::Namespace, 3, 3, &[("", Skip), ("scope", Node), ("name", Str)])
        }
        md::TEMPLATE_TYPE => l(
            "DITemplateTypeParameter",
            DiKind::TemplateTypeParameter,
            3,
            4,
            &[("", Skip), ("name", Str), ("type", Node), ("defaulted", Bool)],
        ),
        md::TEMPLATE_VALUE => l(
            "DITemplateValueParameter",
            DiKind::TemplateValueParameter,
            5,
            6,
            &[
                ("", Skip),
                ("tag", UInt),
                ("name", Str),
                ("type", Node),
                ("defaulted", Bool),
                ("value", Node),
            ],
        ),
        md::GLOBAL_VAR => l(
            "DIGlobalVariable",
            DiKind::GlobalVariable,
            12,
            13,
            &[
                ("", Skip),
                ("scope", Node),
                ("name", Str),
                ("linkageName", Str),
                ("file", Node),
                ("line", UInt),
                ("type", Node),
                ("isLocal", Bool),
                ("isDefinition", Bool),
                ("declaration", Node),
                ("templateParams", List),
                ("align", UInt),
                ("annotations", UnsupMd),
            ],
        ),
        md::LOCAL_VAR => l(
            "DILocalVariable",
            DiKind::LocalVariable,
            9,
            10,
            &[
                ("", Skip),
                ("scope", Node),
                ("name", Str),
                ("file", Node),
                ("line", UInt),
                ("type", Node),
                ("arg", UInt),
                ("flags", UInt),
                ("align", UInt),
                ("annotations", UnsupMd),
            ],
        ),
        md::IMPORTED_ENTITY => l(
            "DIImportedEntity",
            DiKind::ImportedEntity,
            6,
            8,
            &[
                ("", Skip),
                ("tag", UInt),
                ("scope", Node),
                ("entity", Node),
                ("line", UInt),
                ("name", Str),
                ("file", Node),
                ("elements", List),
            ],
        ),
        md::GLOBAL_VAR_EXPR => l(
            "DIGlobalVariableExpression",
            DiKind::GlobalVariableExpression,
            3,
            3,
            &[("", Skip), ("var", Node), ("expr", Expr)],
        ),
        md::LABEL => l(
            "DILabel",
            DiKind::Label,
            5,
            7,
            &[
                ("", Skip),
                ("scope", Node),
                ("name", Str),
                ("file", Node),
                ("line", UInt),
                ("column", UInt),
                ("coroSuspendIdx", Opt),
            ],
        ),
        md::GENERIC_DEBUG => other("GenericDINode"),
        md::OBJC_PROPERTY => other("DIObjCProperty"),
        md::MODULE => other("DIModule"),
        md::MACRO => other("DIMacro"),
        md::MACRO_FILE => other("DIMacroFile"),
        md::STRING_TYPE => other("DIStringType"),
        md::COMMON_BLOCK => other("DICommonBlock"),
        md::GENERIC_SUBRANGE => other("DIGenericSubrange"),
        md::SUBRANGE_TYPE => other("DISubrangeType"),
        md::FIXED_POINT_TYPE => other("DIFixedPointType"),
        _ => None,
    }
}

/// A `DITemplateValueParameter` of five operands, without `defaulted`.
const TEMPLATE_VALUE_5: &[(&str, How)] =
    &[("", Skip), ("tag", UInt), ("name", Str), ("type", Node), ("value", Node)];

/// Whether a record of `code` is a debug node (of any kind).
pub(crate) fn is_node(code: u32) -> bool {
    layout(code).is_some()
}

/// Whether a record of `code` is debug information: a node, a
/// location, an expression or an assign id.
pub(crate) fn is_debug(code: u32) -> bool {
    is_node(code) || matches!(code, md::LOCATION | md::EXPRESSION | md::ASSIGN_ID)
}

/// LLVM's `decodeSignRotatedValue`.
fn unrotate(v: u64) -> u64 {
    if v & 1 == 0 {
        v >> 1
    } else if v != 1 {
        ((v >> 1) as i64).wrapping_neg() as u64
    } else {
        1 << 63
    }
}

/// A slot of the block being read, or of the module's.
#[derive(Clone, Copy, Debug)]
enum Slot<'x, 'a> {
    String(&'a [u8]),
    Rec { code: u32, pos: u64, ops: &'x [u64] },
}

/// The reader of one METADATA block's debug records: the module's, or a
/// function's (with the module's ids in reach, numbered first).
pub(crate) struct Dx<'x, 'a> {
    pub module: &'x mut Module,
    pub values: &'x mut Values,
    pub raw: &'x Raw<'a>,
    /// The module's metadata strings: (start, len) by table and index.
    pub md_text: &'x [Vec<(u32, u32)>],
    pub handles: &'x mut Vec<Handle>,
    /// The function's block; empty at the module level.
    pub lslots: &'x [LSlot<'a>],
    pub lpool: &'x [u64],
    pub lhandles: &'x mut Vec<Handle>,
    pub scratch: &'x mut Scratch<'a>,
    /// The notes the constants made.
    pub notes: u8,
}

impl<'x, 'a> Dx<'x, 'a> {
    fn count(&self) -> u64 {
        self.raw.md.len() as u64
    }

    fn slot(&self, id: u64, at: u64) -> BcResult<Slot<'x, 'a>> {
        let raw: &'x Raw<'a> = self.raw;
        if let Some(slot) = usize::try_from(id).ok().and_then(|i| raw.md.get(i)) {
            return Ok(match *slot {
                MdSlot::String { table, index } => {
                    let md_text: &'x [Vec<(u32, u32)>] = self.md_text;
                    let (start, len) = md_text[table as usize][index as usize];
                    let blob = raw.md_strings[table as usize].blob;
                    Slot::String(&blob[start as usize..][..len as usize])
                }
                MdSlot::Kept(rec) => {
                    Slot::Rec { code: rec.code, pos: rec.pos, ops: rec.ops(&raw.pool) }
                }
            });
        }
        let lslots: &'x [LSlot<'a>] = self.lslots;
        let lpool: &'x [u64] = self.lpool;
        if let Some(&slot) = usize::try_from(id - self.count()).ok().and_then(|i| lslots.get(i)) {
            return Ok(match slot {
                LSlot::String(text) => Slot::String(text),
                LSlot::Rec { code, pos, start, len } => {
                    Slot::Rec { code, pos, ops: &lpool[start as usize..][..len as usize] }
                }
            });
        }
        fail(BcErrorKind::Invalid, at, format!("metadata {id} is not defined"))
    }

    fn handle(&self, id: u64) -> Handle {
        let count = self.count();
        if id < count {
            return self.handles.get(id as usize).copied().unwrap_or(Handle::None);
        }
        self.lhandles.get((id - count) as usize).copied().unwrap_or(Handle::None)
    }

    fn set_handle(&mut self, id: u64, handle: Handle) {
        let count = self.count();
        let slot = if id < count {
            self.handles.get_mut(id as usize)
        } else {
            self.lhandles.get_mut((id - count) as usize)
        };
        if let Some(slot) = slot {
            *slot = handle;
        }
    }

    fn invalid(&mut self) {
        self.module.record_unsupported(INVALID, None);
    }

    /// An empty tuple: what stands for a reference LLVM's verifier
    /// refuses, as in `hod_ll`.
    fn empty_tuple(&mut self, at: u64) -> BcResult<DiId> {
        ir(at, self.module.debug.add(&DiNode::Tuple(DiTuple { elements: &[] })))
    }

    /// Value `value` of the module as a constant.
    fn constant(&mut self, value: u64, at: u64) -> BcResult<Option<hod_ir::ConstId>> {
        if value >= self.values.len() as u64 {
            // A function's value: not a node's operand.
            return Ok(None);
        }
        let raw = self.raw;
        let (built, notes) =
            self.values.constant(self.module, &raw.constants, &raw.pool, value, at)?;
        self.notes |= notes;
        Ok(Some(built))
    }

    // ---- the two passes ----

    /// Pass one over the ids `first..end`: every node and location
    /// reserved, every expression interned.
    pub fn reserve(&mut self, first: u64, end: u64) -> BcResult<()> {
        for id in first..end {
            let Slot::Rec { code, pos, ops } = self.slot(id, 0)? else { continue };
            let handle = match code {
                md::LOCATION => Handle::Loc(ir(pos, self.module.debug.reserve_loc())?),
                md::EXPRESSION => Handle::Expr(self.expression(ops, pos)?),
                md::ASSIGN_ID => Handle::AssignId,
                code if is_node(code) => Handle::Node(ir(pos, self.module.debug.reserve())?),
                _ => continue,
            };
            self.set_handle(id, handle);
        }
        Ok(())
    }

    /// Pass two: every node and location reserved by pass one defined.
    pub fn define(&mut self, first: u64, end: u64) -> BcResult<()> {
        for id in first..end {
            let Slot::Rec { code, pos, ops } = self.slot(id, 0)? else { continue };
            match (self.handle(id), code) {
                (Handle::Loc(loc), md::LOCATION) => self.location(loc, ops, pos)?,
                (Handle::Node(node), code) if is_node(code) => self.node(node, code, ops, pos)?,
                _ => continue,
            }
            self.define_tuples()?;
        }
        Ok(())
    }

    /// The tuples fields named as single nodes, defined.
    fn define_tuples(&mut self) -> BcResult<()> {
        while let Some((node, id)) = self.scratch.tuples.pop() {
            let Slot::Rec { pos, ops, .. } = self.slot(id, 0)? else {
                unreachable!("only a tuple is pushed")
            };
            let mark = self.scratch.lists.len();
            for &op in ops {
                let element = match op.checked_sub(1) {
                    Some(id) => self.node_of(id, pos)?,
                    None => DiId::NONE,
                };
                self.scratch.lists.push(element);
            }
            let tuple = DiNode::Tuple(DiTuple { elements: &self.scratch.lists[mark..] });
            ir(pos, self.module.debug.define(node, &tuple))?;
            self.scratch.lists.truncate(mark);
        }
        Ok(())
    }

    /// `!llvm.dbg.cu`'s operands (metadata ids) listed as compile units.
    pub fn compile_units(&mut self, units: &[u64], at: u64) -> BcResult<()> {
        for &id in units {
            let unit = self.node_of(id, at)?;
            if self.module.debug.find_kind(unit) == Some(DiKind::CompileUnit) {
                ir(at, self.module.debug.add_compile_unit(unit))?;
            } else {
                self.invalid();
            }
        }
        self.define_tuples()
    }

    // ---- references ----

    /// Metadata `id` where a single node is expected.
    pub fn node_of(&mut self, id: u64, at: u64) -> BcResult<DiId> {
        match self.handle(id) {
            Handle::Node(node) => return Ok(node),
            Handle::None => {}
            Handle::Loc(_) | Handle::Expr(_) | Handle::AssignId => {
                // Not a node a field may name; LLVM's verifier refuses it.
                self.invalid();
                return self.empty_tuple(at);
            }
        }
        let node = match self.slot(id, at)? {
            Slot::String(text) => {
                ir(at, self.module.debug.add(&DiNode::String(DiString { text })))?
            }
            Slot::Rec { code: md::NODE | md::DISTINCT_NODE, .. } => {
                let node = ir(at, self.module.debug.reserve())?;
                self.scratch.tuples.push((node, id));
                node
            }
            Slot::Rec { code: md::VALUE, pos, ops } => {
                let &[_, value] = ops else {
                    return fail(BcErrorKind::Invalid, pos, "a METADATA_VALUE record cut short");
                };
                let Some(value) = self.constant(value, pos)? else {
                    // A function's value inside a node.
                    self.invalid();
                    return self.empty_tuple(at);
                };
                ir(at, self.module.debug.add(&DiNode::Constant(DiConstant { value })))?
            }
            Slot::Rec { .. } => {
                self.invalid();
                return self.empty_tuple(at);
            }
        };
        self.set_handle(id, Handle::Node(node));
        Ok(node)
    }

    /// The single node an operand names (metadata id + 1): `None` for 0.
    fn node_ref(&mut self, op: u64, at: u64) -> BcResult<Option<DiId>> {
        match op.checked_sub(1) {
            Some(id) => self.node_of(id, at).map(Some),
            None => Ok(None),
        }
    }

    /// The elements of the tuple an operand names, pushed to `lists`.
    fn list_ref(&mut self, op: u64, at: u64) -> BcResult<(u32, u32)> {
        let mark = self.scratch.lists.len() as u32;
        let Some(id) = op.checked_sub(1) else { return Ok((mark, mark)) };
        let Slot::Rec { code: md::NODE | md::DISTINCT_NODE, ops, .. } = self.slot(id, at)? else {
            // LLVM's verifier wants a tuple here.
            self.invalid();
            return Ok((mark, mark));
        };
        for &op in ops {
            let element = match op.checked_sub(1) {
                Some(id) => self.node_of(id, at)?,
                None => DiId::NONE,
            };
            self.scratch.lists.push(element);
        }
        Ok((mark, self.scratch.lists.len() as u32))
    }

    /// The string an operand names; `None` for 0.
    fn string_ref(&mut self, op: u64, at: u64) -> BcResult<Option<&'a [u8]>> {
        let Some(id) = op.checked_sub(1) else { return Ok(None) };
        match self.slot(id, at)? {
            Slot::String(text) => Ok(Some(text)),
            Slot::Rec { .. } => fail(BcErrorKind::Invalid, at, "expected a metadata string"),
        }
    }

    /// The expression an operand names.
    fn expr_ref(&mut self, op: u64, at: u64) -> BcResult<ExprId> {
        match op.checked_sub(1).map(|id| self.handle(id)) {
            Some(Handle::Expr(expr)) => Ok(expr),
            _ => fail(BcErrorKind::Invalid, at, "expected a DIExpression"),
        }
    }

    /// The integer a constant as metadata holds: sign-extended from its
    /// width when `signed`.
    fn md_int(&mut self, op: u64, signed: bool, at: u64) -> BcResult<Option<u64>> {
        let Some(id) = op.checked_sub(1) else { return Ok(None) };
        let Slot::Rec { code: md::VALUE, ops: &[_, value], pos } = self.slot(id, at)? else {
            return Ok(None);
        };
        let Some(built) = self.constant(value, pos)? else { return Ok(None) };
        let Some(bits) = self.module.consts.int_value(built) else { return Ok(None) };
        let width = self.module.types.int_bits(self.module.consts.ty(built)).unwrap_or(64);
        if signed && (1..64).contains(&width) && bits >> (width - 1) & 1 == 1 {
            return Ok(Some(bits | (u64::MAX << width)));
        }
        Ok(Some(bits))
    }

    /// A subrange bound (metadata id + 1): a constant integer, a node or
    /// an expression.
    fn bound(&mut self, op: u64, at: u64) -> BcResult<F<'a>> {
        let id = op - 1;
        if let Handle::Expr(expr) = self.handle(id) {
            return Ok(F::Expr(expr));
        }
        if let Some(value) = self.md_int(op, true, at)? {
            return Ok(F::Int(value as i64));
        }
        Ok(F::Node(Some(self.node_of(id, at)?)))
    }

    /// Whether an operand that names metadata names nothing: null, `!{}`
    /// or `""`.
    fn md_default(&self, op: u64) -> bool {
        let Some(id) = op.checked_sub(1) else { return true };
        match self.slot(id, 0) {
            Ok(Slot::String(text)) => text.is_empty(),
            Ok(Slot::Rec { code: md::NODE | md::DISTINCT_NODE, ops, .. }) => ops.is_empty(),
            _ => false,
        }
    }

    // ---- records ----

    /// An EXPRESSION record, interned.
    fn expression(&mut self, ops: &[u64], at: u64) -> BcResult<ExprId> {
        let Some((&flags, elements)) = ops.split_first() else {
            return fail(BcErrorKind::Invalid, at, "an EXPRESSION record cut short");
        };
        // Version 3 is LLVM 19 to 22's; older ones need upgrades LLVM
        // makes as it reads them.
        if flags >> 1 != 3 {
            return fail(LAYOUT, at, format!("a DIExpression of version {}", flags >> 1));
        }
        ir(at, self.module.debug.expr_from_elements(elements))
    }

    /// A LOCATION record, defining `loc`.
    fn location(&mut self, loc: LocId, ops: &[u64], at: u64) -> BcResult<()> {
        match ops.len() {
            5 | 6 | 8 => {}
            n if n > 8 => {
                return fail(LAYOUT, at, format!("a LOCATION record of {n} operands"));
            }
            _ => return fail(BcErrorKind::Invalid, at, "a LOCATION record of the wrong length"),
        }
        let scope = self.node_of(ops[3], at)?;
        let inlined_at = match ops[4].checked_sub(1) {
            None => LocId::NONE,
            Some(id) => {
                if let Handle::Loc(loc) = self.handle(id) {
                    loc
                } else {
                    self.slot(id, at)?;
                    self.invalid();
                    LocId::NONE
                }
            }
        };
        let location =
            DiLocation { line: ops[1] as u32, column: column(ops[2]), scope, inlined_at };
        let debug = &mut self.module.debug;
        let made = if ops[0] != 0 {
            debug.define_distinct_loc(loc, location)
        } else {
            debug.define_loc(loc, location)
        };
        ir(at, made)
    }

    /// A node record of `code`, defining `node`.
    #[allow(clippy::too_many_lines, clippy::match_same_arms)] // Each kind's special operands.
    fn node(&mut self, node: DiId, code: u32, ops: &[u64], at: u64) -> BcResult<()> {
        let layout = layout(code).expect("a node's code has a layout");
        let name = layout.name;
        if ops.len() < layout.min {
            return fail(BcErrorKind::Invalid, at, format!("a {name} record cut short"));
        }
        if ops.len() > layout.max {
            return fail(
                LAYOUT,
                at,
                format!("a {name} record of {} operands, more than LLVM 22 writes", ops.len()),
            );
        }
        let distinct = ops[0] & 1 != 0;
        let Some(kind) = layout.kind else {
            let other = DiNode::Other(DiOther { kind: name.as_bytes() });
            let debug = &mut self.module.debug;
            let made = if distinct {
                debug.define_distinct(node, &other)
            } else {
                debug.define(node, &other)
            };
            return ir(at, made);
        };
        let flags = ops[0];
        let mut fields = layout.fields;
        let mut size_md = false;
        let mut skip: u64 = 0;
        let old = |what: &str| fail(LAYOUT, at, format!("a {name} record {what}"));
        self.scratch.fields.clear();
        match code {
            md::BASIC_TYPE | md::DERIVED_TYPE => size_md = flags & 2 != 0,
            md::COMPOSITE_TYPE => {
                if flags & 2 == 0 {
                    return old("with type references of a bitcode before LLVM 3.9");
                }
                size_md = flags & 4 != 0;
            }
            md::SUBROUTINE_TYPE if flags & 2 == 0 => {
                return old("with type references of a bitcode before LLVM 3.9");
            }
            md::SUBPROGRAM if flags & 6 != 6 => {
                return old("without the unit and flags of LLVM 8 and later");
            }
            md::GLOBAL_VAR if flags >> 1 != 2 => {
                return old(&format!("of version {}", flags >> 1));
            }
            md::LOCAL_VAR if flags & 2 == 0 => return old("without its alignment"),
            md::SUBRANGE if flags >> 1 != 2 => return old(&format!("of version {}", flags >> 1)),
            // A checksum is its kind and its value, both or neither.
            md::FILE if !(ops.len() > 4 && ops[3] != 0 && ops[4] != 0) => {
                skip |= 1 << 3 | 1 << 4;
            }
            md::COMPILE_UNIT => {
                if ops[1] >> 63 != 0 {
                    // DWARF 6's language name and version.
                    self.unsupported(name, "sourceLanguageName");
                } else {
                    self.scratch.fields.push(("language", F::UInt(ops[1])));
                }
                if ops.get(11).is_some_and(|&op| op != 0) {
                    return old("with its subprograms, of a bitcode before LLVM 3.9");
                }
            }
            md::NAMESPACE => self.scratch.fields.push(("exportSymbols", F::Bool(flags & 2 != 0))),
            md::LABEL => self.scratch.fields.push(("isArtificial", F::Bool(flags & 2 != 0))),
            md::TEMPLATE_VALUE if ops.len() == 5 => fields = TEMPLATE_VALUE_5,
            md::ENUMERATOR => self.enumerator(ops, at)?,
            _ => {}
        }
        for (i, (&op, &(field, how))) in ops.iter().zip(fields).enumerate() {
            if skip >> i & 1 == 1 {
                continue;
            }
            let value = match how {
                Skip | Drop => continue,
                UInt => F::UInt(op),
                Bool => F::Bool(op != 0),
                Node => F::Node(self.node_ref(op, at)?),
                List => {
                    let (start, end) = self.list_ref(op, at)?;
                    F::List(start, end)
                }
                Str => match self.string_ref(op, at)? {
                    Some(text) => F::Str(text),
                    None => continue,
                },
                Expr => F::Expr(self.expr_ref(op, at)?),
                Bound if op == 0 => continue,
                Bound => self.bound(op, at)?,
                Size if !size_md => F::UInt(op),
                Size if op == 0 => continue,
                Size => match self.md_int(op, false, at)? {
                    Some(value) => F::UInt(value),
                    None => {
                        return fail(
                            BcErrorKind::Invalid,
                            at,
                            format!("a {name}.{field} that is not a constant"),
                        );
                    }
                },
                PlusOne if op == 0 => continue,
                PlusOne => F::UInt(op - 1),
                Opt if op == u64::MAX => continue,
                Opt => F::UInt(op),
                UnsupInt(default) => {
                    if op != default {
                        self.unsupported(name, field);
                    }
                    continue;
                }
                UnsupMd => {
                    if !self.md_default(op) {
                        self.unsupported(name, field);
                    }
                    continue;
                }
                PtrAuth => {
                    for (field, bits) in [
                        ("ptrAuthKey", op & 0xF),
                        ("ptrAuthIsAddressDiscriminated", op >> 4 & 1),
                        ("ptrAuthExtraDiscriminator", op >> 5 & 0xFFFF),
                        ("ptrAuthIsaPointer", op >> 21 & 1),
                        ("ptrAuthAuthenticatesNullValues", op >> 22 & 1),
                    ] {
                        if bits != 0 {
                            self.unsupported(name, field);
                        }
                    }
                    continue;
                }
            };
            self.scratch.fields.push((field, value));
        }
        let scratch = &*self.scratch;
        let mut built = DiNode::llvm_default(kind);
        for &(field, value) in &scratch.fields {
            let value = match value {
                F::UInt(value) => DiFieldValue::UInt(value),
                F::Int(value) => DiFieldValue::Int(value),
                F::Bool(value) => DiFieldValue::Bool(value),
                F::Str(text) => DiFieldValue::Str(text),
                F::Node(id) => DiFieldValue::Node(id),
                F::List(start, end) => {
                    DiFieldValue::List(&scratch.lists[start as usize..end as usize])
                }
                F::Expr(expr) => DiFieldValue::Expr(expr),
            };
            ir(at, built.set(field.as_bytes(), value))?;
        }
        let debug = &mut self.module.debug;
        let made =
            if distinct { debug.define_distinct(node, &built) } else { debug.define(node, &built) };
        ir(at, made)?;
        self.scratch.fields.clear();
        self.scratch.lists.clear();
        Ok(())
    }

    /// An ENUMERATOR's fields: `[flags, value, name]`, or with a wide
    /// value (LLVM 12 and later) `[flags, bit width, name, words...]`,
    /// each word sign-rotated.
    fn enumerator(&mut self, ops: &[u64], at: u64) -> BcResult<()> {
        let flags = ops[0];
        let unsigned = flags & 2 != 0;
        let (width, name, words) = if flags & 4 != 0 {
            if ops.len() < 4 {
                return fail(BcErrorKind::Invalid, at, "a DIEnumerator record cut short");
            }
            (ops[1], ops[2], &ops[3..])
        } else {
            if ops.len() != 3 {
                return fail(BcErrorKind::Invalid, at, "a DIEnumerator record of the wrong length");
            }
            (64, ops[2], &ops[1..2])
        };
        if width == 0 || width > u64::from(hod_ir::MAX_INT_BITS) {
            return fail(BcErrorKind::Invalid, at, "a DIEnumerator of no integer width");
        }
        if let Some(text) = self.string_ref(name, at)? {
            self.scratch.fields.push(("name", F::Str(text)));
        }
        let value = wide_value(width, unsigned, words).unwrap_or_else(|| {
            self.module.record_unsupported("debug:DIEnumerator.wide", None);
            F::UInt(0)
        });
        self.scratch.fields.push(("value", value));
        self.scratch.fields.push(("isUnsigned", F::Bool(unsigned)));
        Ok(())
    }

    /// Records that a field of `kind` hod cannot represent is set.
    fn unsupported(&mut self, kind: &str, field: &str) {
        match di_field(kind.as_bytes(), field.as_bytes()) {
            Some(DiMapping::Unsupported(construct)) => {
                self.module.record_unsupported(construct, None);
            }
            _ => self.module.record_unsupported(&format!("debug:{kind}.{field}"), None),
        }
    }
}

/// LLVM's `adjustColumn`: a column of more than 16 bits is none.
pub(crate) fn column(op: u64) -> u16 {
    u16::try_from(op).unwrap_or(0)
}

/// An enumerator's value of `width` bits from its sign-rotated words, as
/// `hod_ll` reads the decimal `llvm-dis` prints for it; `None` for a
/// value that does not fit 64 bits.
fn wide_value(width: u64, unsigned: bool, words: &[u64]) -> Option<F<'static>> {
    let limbs = width.div_ceil(64) as usize;
    let top_bits = width - (limbs as u64 - 1) * 64;
    let top_mask = if top_bits == 64 { u64::MAX } else { (1u64 << top_bits) - 1 };
    let limb = |i: usize| {
        let word = words.get(i).map_or(0, |&w| unrotate(w));
        if i + 1 == limbs { word & top_mask } else { word }
    };
    let negative = !unsigned && limb(limbs - 1) >> (top_bits - 1) & 1 == 1;
    if limbs == 1 {
        let low = limb(0);
        if negative {
            let extended = if width == 64 { low } else { low | (u64::MAX << width) };
            return Some(F::Int(extended as i64));
        }
        return Some(F::UInt(low));
    }
    let mut rest = (1..limbs).map(limb);
    if negative {
        let ones = rest
            .enumerate()
            .all(|(i, word)| word == if i + 2 == limbs { top_mask } else { u64::MAX });
        let low = limb(0);
        return (ones && low >> 63 == 1).then_some(F::Int(low as i64));
    }
    let zeros = rest.all(|word| word == 0);
    zeros.then(|| F::UInt(limb(0)))
}

/// Reads a function's METADATA block (just entered) into `block`: its
/// strings and its records, by id.
pub(crate) fn read_local_block<'a>(
    cursor: &mut Cursor<'a, '_>,
    ops: &mut Vec<u64>,
    block: &mut LocalBlock<'a>,
) -> BcResult<()> {
    loop {
        match cursor.next()? {
            Entry::End => return Ok(()),
            Entry::Block(_) => {
                return fail(BcErrorKind::Invalid, cursor.entry_pos(), "a block inside METADATA");
            }
            Entry::Record(abbrev) => {
                let at = cursor.entry_pos();
                let (code, blob) = cursor.record(abbrev, ops)?;
                match code {
                    md::STRINGS => strings(ops, blob, at, &mut block.slots)?,
                    md::INDEX_OFFSET | md::INDEX => {}
                    code => match md::defines_id(code) {
                        Some(true) => {
                            let (Ok(start), Ok(len)) =
                                (u32::try_from(block.pool.len()), u32::try_from(ops.len()))
                            else {
                                return fail(
                                    BcErrorKind::Limit,
                                    at,
                                    "a METADATA block over 2^32 words",
                                );
                            };
                            block.pool.extend_from_slice(ops);
                            block.slots.push(LSlot::Rec { code, pos: at, start, len });
                        }
                        Some(false) => {
                            return fail(
                                BcErrorKind::Invalid,
                                at,
                                format!("record code {code} in a function's METADATA block"),
                            );
                        }
                        None => {
                            return fail(
                                BcErrorKind::Unsupported("record"),
                                at,
                                format!("record code {code} in a METADATA block"),
                            );
                        }
                    },
                }
            }
        }
    }
}

/// A STRINGS record's strings, pushed as slots.
fn strings<'a>(
    ops: &[u64],
    blob: Option<&'a [u8]>,
    at: u64,
    slots: &mut Vec<LSlot<'a>>,
) -> BcResult<()> {
    let (&[count, offset], Some(blob)) = (ops, blob) else {
        return fail(
            BcErrorKind::Invalid,
            at,
            "a METADATA_STRINGS record without its count, offset and blob",
        );
    };
    if count > blob.len() as u64 * 8 / 6 || offset > blob.len() as u64 {
        return fail(BcErrorKind::Invalid, at, "a METADATA_STRINGS count or offset past its blob");
    }
    let mut bits = BitReader::new(blob);
    if bits.seek(0, offset * 8).is_err() {
        return fail(BcErrorKind::Invalid, at, "a METADATA_STRINGS offset past its blob");
    }
    let mut start = offset;
    for _ in 0..count {
        let Ok(len) = bits.vbr(6) else {
            return fail(BcErrorKind::Invalid, at, "METADATA_STRINGS lengths cut short");
        };
        let end = start.saturating_add(len);
        if end > blob.len() as u64 {
            return fail(BcErrorKind::Invalid, at, "a metadata string past its blob");
        }
        slots.push(LSlot::String(&blob[start as usize..end as usize]));
        start = end;
    }
    Ok(())
}

/// What a function block holds of debug information: its METADATA
/// block, and the metadata id its definition's `!dbg` names (the last).
pub(crate) fn scan_function<'a>(
    data: &'a [u8],
    info: &BlockInfo,
    span: BlockSpan,
    dbg_kind: Option<u64>,
    ops: &mut Vec<u64>,
    block: &mut LocalBlock<'a>,
) -> BcResult<Vec<u64>> {
    let mut cursor = Cursor::enter_at(data, info, span)?;
    let mut dbg = Vec::new();
    loop {
        match cursor.next()? {
            Entry::End => return Ok(dbg),
            Entry::Block(block::METADATA) => {
                cursor.enter()?;
                read_local_block(&mut cursor, ops, block)?;
            }
            Entry::Block(block::METADATA_ATTACHMENT) => {
                cursor.enter()?;
                loop {
                    match cursor.next()? {
                        Entry::End => break,
                        Entry::Block(_) => cursor.skip(),
                        Entry::Record(abbrev) => {
                            let (code, _) = cursor.record(abbrev, ops)?;
                            if code != md::ATTACHMENT || !ops.len().is_multiple_of(2) {
                                continue;
                            }
                            for pair in ops.chunks(2) {
                                if Some(pair[0]) == dbg_kind {
                                    dbg.push(pair[1]);
                                }
                            }
                        }
                    }
                }
            }
            Entry::Block(_) => cursor.skip(),
            Entry::Record(abbrev) => {
                cursor.record(abbrev, ops)?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! `spec/bc.md` §6.3 and §6.4: the layout table against `hod_ir`'s
    //! field mapping, and each named refusal of a record the table does
    //! not know.

    use super::*;
    use crate::module::with_dx;

    const MODULE: &[u8] = include_bytes!("../tests/data/debug-llvm22.bc");

    fn codes() -> impl Iterator<Item = (u32, Layout)> {
        (0..128).filter_map(|code| layout(code).map(|l| (code, l)))
    }

    fn noted(dx: &Dx<'_, '_>, construct: &str) -> bool {
        dx.module.unsupported().any(|u| u.construct == construct)
    }

    /// §6.3: every field the table names is one `hod_ir` maps, drops or
    /// refuses as the table says.
    #[test]
    fn the_layouts_agree_with_hod_ir() {
        let mut checked = 0;
        for (_, l) in codes() {
            if l.kind.is_none() {
                continue;
            }
            for &(field, how) in l.fields {
                if field.is_empty() || matches!(how, Skip | PtrAuth) {
                    continue;
                }
                let mapping = di_field(l.name.as_bytes(), field.as_bytes());
                let ok = match how {
                    Drop => matches!(mapping, Some(DiMapping::Dropped(_))),
                    UnsupInt(_) | UnsupMd => matches!(mapping, Some(DiMapping::Unsupported(_))),
                    _ => mapping == Some(DiMapping::Mapped),
                };
                assert!(ok, "{}.{field}: {how:?} but hod_ir says {mapping:?}", l.name);
                checked += 1;
            }
        }
        assert!(checked > 100, "{checked} fields");
    }

    /// §6.4: a record one operand short is invalid; one operand longer
    /// than LLVM 22 writes is `debug-layout`.
    #[test]
    fn record_lengths_outside_the_layout_are_refused() {
        with_dx(MODULE, |dx| {
            for (code, l) in codes() {
                if l.kind.is_none() {
                    continue;
                }
                let node = dx.module.debug.reserve().expect("reserve");
                let short = vec![0; l.min - 1];
                let e = dx.node(node, code, &short, 0).expect_err(l.name);
                assert_eq!(e.kind, BcErrorKind::Invalid, "{}: {e}", l.name);
                if l.max == usize::MAX {
                    continue; // DIEnumerator: any number of words.
                }
                let long = vec![0; l.max + 1];
                let e = dx.node(node, code, &long, 0).expect_err(l.name);
                assert_eq!(e.kind, LAYOUT, "{}: {e}", l.name);
            }
        });
    }

    /// §6.4: versions LLVM 19 to 22 do not write are `debug-layout`.
    #[test]
    fn unknown_versions_are_refused() {
        with_dx(MODULE, |dx| {
            let e = dx.expression(&[4 << 1], 0).expect_err("DIExpression version 4");
            assert_eq!(e.kind, LAYOUT);
            let e = dx.expression(&[], 0).expect_err("an empty EXPRESSION");
            assert_eq!(e.kind, BcErrorKind::Invalid);
            let node = dx.module.debug.reserve().expect("reserve");
            let mut ops = vec![0; 13];
            ops[0] = 1 << 1;
            let e = dx.node(node, md::GLOBAL_VAR, &ops, 0).expect_err("DIGlobalVariable v1");
            assert_eq!(e.kind, LAYOUT);
            let mut ops = vec![0; 5];
            ops[0] = 1 << 1;
            let e = dx.node(node, md::SUBRANGE, &ops, 0).expect_err("DISubrange v1");
            assert_eq!(e.kind, LAYOUT);
            let loc = dx.module.debug.reserve_loc().expect("reserve");
            let e = dx.location(loc, &[0; 9], 0).expect_err("a LOCATION of 9");
            assert_eq!(e.kind, LAYOUT);
            let e = dx.location(loc, &[0; 4], 0).expect_err("a LOCATION of 4");
            assert_eq!(e.kind, BcErrorKind::Invalid);
        });
    }

    /// §6.4: an operand naming metadata that does not exist is an error.
    #[test]
    fn out_of_range_operands_are_errors() {
        with_dx(MODULE, |dx| {
            let node = dx.module.debug.reserve().expect("reserve");
            let mut ops = vec![0; 12];
            ops[1] = 0x0F; // DW_TAG_pointer_type
            ops[6] = 1 << 40; // baseType
            let e = dx.node(node, md::DERIVED_TYPE, &ops, 0).expect_err("baseType");
            assert_eq!(e.kind, BcErrorKind::Invalid, "{e}");
            let loc = dx.module.debug.reserve_loc().expect("reserve");
            let e = dx.location(loc, &[0, 1, 1, 1 << 40, 0], 0).expect_err("scope");
            assert_eq!(e.kind, BcErrorKind::Invalid, "{e}");
        });
    }

    /// An unknown record code is neither debug information nor a
    /// metadata id: the block reader skips it.
    #[test]
    fn unknown_codes_are_not_debug_records() {
        assert!(!is_debug(99) && !is_node(99));
        assert_eq!(md::defines_id(99), None);
    }

    /// §6.3: an enumerator wider than 64 bits is noted, not guessed.
    #[test]
    fn wide_enumerators_are_noted() {
        with_dx(MODULE, |dx| {
            let node = dx.module.debug.reserve().expect("reserve");
            // flags: wide; 128 bits; no name; two words.
            dx.node(node, md::ENUMERATOR, &[4, 128, 0, 2, 2], 0).expect("a wide enumerator");
            assert!(noted(dx, "debug:DIEnumerator.wide"));
        });
    }
}
