//! Metadata (`spec/ll.md` §7): the index of numbered nodes and named
//! metadata, and the three named lists the module level reads — module
//! flags, linker options and dependent libraries.

use hod_ir::{Arch, FlagValue};

use crate::Span;
use crate::error::{LlErrorKind, PResult, fail, ir_fail};
use crate::index::parse_u32;
use crate::lexer::Tok;
use crate::parser::{Parser, unquote};

/// What a numbered metadata node is, from the word after its `!`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum MdKind {
    /// `!{…}`: a generic tuple.
    Tuple,
    /// `!DILocation`.
    Location,
    /// `!DIExpression`.
    Expression,
    /// `!DIGlobalVariableExpression`.
    GlobalVariableExpression,
    /// `!GenericDINode`.
    GenericNode,
    /// `!DISubrange`.
    Subrange,
    /// `!DIEnumerator`.
    Enumerator,
    /// `!DIBasicType`.
    BasicType,
    /// `!DIDerivedType`.
    DerivedType,
    /// `!DICompositeType`.
    CompositeType,
    /// `!DISubroutineType`.
    SubroutineType,
    /// `!DIFile`.
    File,
    /// `!DICompileUnit`.
    CompileUnit,
    /// `!DISubprogram`.
    Subprogram,
    /// `!DILexicalBlock`.
    LexicalBlock,
    /// `!DILexicalBlockFile`.
    LexicalBlockFile,
    /// `!DINamespace`.
    Namespace,
    /// `!DIModule`.
    Module,
    /// `!DITemplateTypeParameter`.
    TemplateTypeParameter,
    /// `!DITemplateValueParameter`.
    TemplateValueParameter,
    /// `!DIGlobalVariable`.
    GlobalVariable,
    /// `!DILocalVariable`.
    LocalVariable,
    /// `!DILabel`.
    Label,
    /// `!DIObjCProperty`.
    ObjCProperty,
    /// `!DIImportedEntity`.
    ImportedEntity,
    /// `!DIAssignID`.
    AssignId,
    /// `!DIMacro`.
    Macro,
    /// `!DIMacroFile`.
    MacroFile,
    /// `!DICommonBlock`.
    CommonBlock,
    /// `!DIArgList`.
    ArgList,
    /// `!DIStringType`.
    StringType,
    /// `!DIGenericSubrange`.
    GenericSubrange,
    /// `!DIFixedPointType` (LLVM 21 and later).
    FixedPointType,
    /// `!DISubrangeType` (LLVM 21 and later).
    SubrangeType,
    /// A specialized node this list does not have.
    Other,
}

impl MdKind {
    /// The kind of the specialized node called `name` (`b"DILocation"`).
    pub fn from_name(name: &[u8]) -> MdKind {
        match name {
            b"DILocation" => MdKind::Location,
            b"DIExpression" => MdKind::Expression,
            b"DIGlobalVariableExpression" => MdKind::GlobalVariableExpression,
            b"GenericDINode" => MdKind::GenericNode,
            b"DISubrange" => MdKind::Subrange,
            b"DIEnumerator" => MdKind::Enumerator,
            b"DIBasicType" => MdKind::BasicType,
            b"DIDerivedType" => MdKind::DerivedType,
            b"DICompositeType" => MdKind::CompositeType,
            b"DISubroutineType" => MdKind::SubroutineType,
            b"DIFile" => MdKind::File,
            b"DICompileUnit" => MdKind::CompileUnit,
            b"DISubprogram" => MdKind::Subprogram,
            b"DILexicalBlock" => MdKind::LexicalBlock,
            b"DILexicalBlockFile" => MdKind::LexicalBlockFile,
            b"DINamespace" => MdKind::Namespace,
            b"DIModule" => MdKind::Module,
            b"DITemplateTypeParameter" => MdKind::TemplateTypeParameter,
            b"DITemplateValueParameter" => MdKind::TemplateValueParameter,
            b"DIGlobalVariable" => MdKind::GlobalVariable,
            b"DILocalVariable" => MdKind::LocalVariable,
            b"DILabel" => MdKind::Label,
            b"DIObjCProperty" => MdKind::ObjCProperty,
            b"DIImportedEntity" => MdKind::ImportedEntity,
            b"DIAssignID" => MdKind::AssignId,
            b"DIMacro" => MdKind::Macro,
            b"DIMacroFile" => MdKind::MacroFile,
            b"DICommonBlock" => MdKind::CommonBlock,
            b"DIArgList" => MdKind::ArgList,
            b"DIStringType" => MdKind::StringType,
            b"DIGenericSubrange" => MdKind::GenericSubrange,
            b"DIFixedPointType" => MdKind::FixedPointType,
            b"DISubrangeType" => MdKind::SubrangeType,
            _ => MdKind::Other,
        }
    }
}

/// One numbered metadata node: `!id = [distinct] …`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MdNode {
    /// Its number.
    pub id: u32,
    /// What it is.
    pub kind: MdKind,
    /// Whether it is `distinct`.
    pub distinct: bool,
    /// The source between its braces or parentheses.
    pub span: Span,
}

/// One named metadata: `!name = !{…}`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NamedMetadata {
    /// The name as written, without the `!` (escapes not undone).
    pub name: Span,
    /// The source between its braces.
    pub span: Span,
}

/// A metadata operand that is a node: by number, or written in place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MdRef {
    /// `!42`.
    Node(u32),
    /// `!{…}` or `!DIKind(…)` in place: the source between its braces or
    /// parentheses, and its kind.
    Inline(MdKind, Span),
}

/// Every metadata node and named metadata of a module, by number and in
/// file order. Read-only.
#[derive(Clone, Debug, Default)]
pub struct MetadataIndex {
    nodes: Vec<MdNode>,
    /// Node number to index in `nodes` plus one, when the numbers are
    /// dense (they are, in anything a compiler wrote).
    dense: Vec<u32>,
    /// Otherwise `(number, index)`, sorted.
    sparse: Vec<(u32, u32)>,
    named: Vec<NamedMetadata>,
    debug_info: bool,
}

impl MetadataIndex {
    /// The node numbered `id`.
    pub fn node(&self, id: u32) -> Option<&MdNode> {
        let at = if self.sparse.is_empty() {
            (*self.dense.get(id as usize)?).checked_sub(1)?
        } else {
            let found = self.sparse.binary_search_by_key(&id, |&(number, _)| number).ok()?;
            self.sparse[found].1
        };
        Some(&self.nodes[at as usize])
    }

    /// Every node, in file order.
    pub fn nodes(&self) -> &[MdNode] {
        &self.nodes
    }

    /// Every named metadata, in file order.
    pub fn named(&self) -> &[NamedMetadata] {
        &self.named
    }

    /// Whether the module has `!llvm.dbg.cu`.
    pub fn has_debug_info(&self) -> bool {
        self.debug_info
    }
}

/// What LLVM's reader does to the functions of an AArch64 module whose
/// branch-protection module flags are in the old form (§7.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // One for each of six module flags.
pub(crate) struct Upgrade {
    pub bte: bool,
    pub pauth_lr: bool,
    pub gcs: bool,
    pub sign: bool,
    pub sign_all: bool,
    pub sign_b_key: bool,
}

impl Parser<'_> {
    /// Builds the node index.
    pub(crate) fn index_metadata(&mut self) -> PResult<()> {
        let count = self.index.nodes.len();
        let max = self.index.nodes.iter().map(|node| node.id).max().unwrap_or(0) as usize;
        let dense = max < 4 * count + 1024;
        let mut md = MetadataIndex::default();
        if dense {
            md.dense = vec![0; max + 1];
        }
        for (at, node) in self.index.nodes.iter().enumerate() {
            md.nodes.push(MdNode {
                id: node.id,
                kind: node.kind,
                distinct: node.distinct,
                span: node.span,
            });
            if dense {
                if md.dense[node.id as usize] != 0 {
                    return redefined(node.at, node.id);
                }
                md.dense[node.id as usize] = at as u32 + 1;
            } else {
                md.sparse.push((node.id, at as u32));
            }
        }
        md.sparse.sort_unstable();
        if let Some(pair) = md.sparse.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return redefined(self.index.nodes[pair[1].1 as usize].at, pair[1].0);
        }
        md.named = self
            .index
            .named
            .iter()
            .map(|ent| NamedMetadata {
                name: Span { start: ent.name.start + 1, end: ent.name.end },
                span: ent.span,
            })
            .collect();
        self.md = md;
        Ok(())
    }

    /// Reads the named metadata the module level knows.
    pub(crate) fn named_metadata(&mut self) -> PResult<()> {
        let mut upgrade = Some(Upgrade::default());
        for at in 0..self.index.named.len() {
            let ent = self.index.named[at];
            let raw = &self.src[ent.name.start as usize + 1..ent.name.end as usize];
            let name = unquote(raw);
            match &*name {
                b"llvm.module.flags" => {
                    for id in self.node_list(ent.span)? {
                        self.module_flag(id, &mut upgrade)?;
                    }
                }
                b"llvm.linker.options" => {
                    for id in self.node_list(ent.span)? {
                        let strings = self.string_tuple(id)?;
                        let refs: Vec<&[u8]> = strings.iter().map(Vec::as_slice).collect();
                        self.module
                            .add_linker_option(&refs)
                            .map_err(|e| ir_fail(ent.name.start, e))?;
                    }
                }
                b"llvm.dependent-libraries" => {
                    for id in self.node_list(ent.span)? {
                        let strings = self.string_tuple(id)?;
                        let [library] = strings.as_slice() else {
                            return fail(
                                LlErrorKind::Syntax,
                                ent.name.start,
                                "an operand of !llvm.dependent-libraries is one string",
                            );
                        };
                        self.module
                            .add_dependent_library(library)
                            .map_err(|e| ir_fail(ent.name.start, e))?;
                    }
                }
                b"llvm.ident" | b"llvm.errno.tbaa" => {}
                b"llvm.dbg.cu" => {
                    self.md.debug_info = true;
                    self.module.record_unsupported("debug-info", None);
                }
                other => {
                    let construct = format!("named-metadata:{}", String::from_utf8_lossy(other));
                    self.module.record_unsupported(&construct, None);
                }
            }
        }
        if self.module.target().arch() == Arch::Aarch64 {
            self.upgrade = upgrade;
        }
        Ok(())
    }

    /// The node numbers in `!{!1, !2}`, whose inside is `span`.
    fn node_list(&mut self, span: Span) -> PResult<Vec<u32>> {
        let mut ids = Vec::new();
        self.goto(span.start);
        while self.tok.start < span.end {
            self.expect(Tok::Exclaim, "expected a metadata node number")?;
            let number = self.expect(Tok::Int, "expected a metadata node number")?;
            let Some(id) = parse_u32(self.tok_text(number)) else {
                return fail(LlErrorKind::Syntax, number.start, "expected a metadata node number");
            };
            ids.push(id);
            if self.tok.start < span.end {
                self.expect(Tok::Comma, "expected ',' between operands")?;
            }
        }
        Ok(ids)
    }

    /// The inside of the tuple numbered `id`.
    fn tuple(&mut self, id: u32) -> PResult<Span> {
        let at = self.tok.start;
        match self.md.node(id) {
            Some(node) if node.kind == MdKind::Tuple => Ok(node.span),
            Some(_) => fail(LlErrorKind::Syntax, at, format!("metadata !{id} is not a tuple")),
            None => fail(LlErrorKind::Undefined, at, format!("use of undefined metadata '!{id}'")),
        }
    }

    /// The strings of the tuple `!{!"a", !"b"}` numbered `id`.
    fn string_tuple(&mut self, id: u32) -> PResult<Vec<Vec<u8>>> {
        let span = self.tuple(id)?;
        let mut strings = Vec::new();
        self.goto(span.start);
        while self.tok.start < span.end {
            self.expect(Tok::Exclaim, "expected a metadata string")?;
            let text = self.expect(Tok::Str, "expected a metadata string")?;
            strings.push(self.string(text).into_owned());
            if self.tok.start < span.end {
                self.expect(Tok::Comma, "expected ',' between operands")?;
            }
        }
        Ok(strings)
    }

    /// One module flag: `!{i32 behavior, !"name", value}`.
    fn module_flag(&mut self, id: u32, upgrade: &mut Option<Upgrade>) -> PResult<()> {
        let span = self.tuple(id)?;
        self.goto(span.start);
        let at = self.tok.start;
        let behavior = self.parse_typed_const()?;
        let Some(behavior) = self.module.consts.int_value(behavior) else {
            return fail(LlErrorKind::Syntax, at, "a module flag's behavior is an integer");
        };
        self.expect(Tok::Comma, "expected ',' between operands")?;
        self.expect(Tok::Exclaim, "expected the module flag's name")?;
        let name = self.expect(Tok::Str, "expected the module flag's name")?;
        let name = self.string(name).into_owned();
        self.expect(Tok::Comma, "expected ',' between operands")?;
        let mut int = None;
        let mut text = None;
        if self.tok.kind == Tok::Exclaim {
            self.bump();
            if self.tok.kind == Tok::Str {
                text = Some(self.string(self.tok).into_owned());
                self.bump();
            } else {
                // A node, in place or by number: kept as "other".
                self.goto(span.end);
            }
        } else if self.at_word(b"null") {
            self.bump();
        } else {
            let value = self.parse_typed_const()?;
            int = self.module.consts.int_value(value);
        }
        if self.tok.start < span.end {
            return self.syntax("a module flag has three operands");
        }
        let value = match (int, &text) {
            (Some(value), _) => FlagValue::Int(value),
            (None, Some(text)) => FlagValue::Str(text),
            (None, None) => FlagValue::Other,
        };
        let behavior = u32::try_from(behavior).unwrap_or(u32::MAX);
        self.module.set_module_flag(behavior, &name, value).map_err(|e| ir_fail(at, e))?;
        if let Some(value) = int {
            let field = upgrade.as_mut().and_then(|up| match name.as_slice() {
                b"branch-target-enforcement" => Some(&mut up.bte),
                b"branch-protection-pauth-lr" => Some(&mut up.pauth_lr),
                b"guarded-control-stack" => Some(&mut up.gcs),
                b"sign-return-address" => Some(&mut up.sign),
                b"sign-return-address-all" => Some(&mut up.sign_all),
                b"sign-return-address-with-bkey" => Some(&mut up.sign_b_key),
                _ => None,
            });
            if let Some(field) = field {
                if value == 2 {
                    *upgrade = None;
                } else {
                    *field = value == 1;
                }
            }
        }
        Ok(())
    }

    /// A metadata operand that is a node: `!42`, `!{…}` or `!DIKind(…)`.
    pub(crate) fn md_ref(&mut self) -> PResult<MdRef> {
        let at = self.tok.start;
        match self.tok.kind {
            Tok::Exclaim => {
                self.bump();
                if self.tok.kind == Tok::LBrace {
                    return Ok(MdRef::Inline(MdKind::Tuple, self.skip_brackets()?));
                }
                let number = self.expect(Tok::Int, "expected a metadata node")?;
                let Some(id) = parse_u32(self.tok_text(number)) else {
                    return fail(LlErrorKind::Syntax, at, "expected a metadata node number");
                };
                if self.md.node(id).is_none() {
                    let message = format!("use of undefined metadata '!{id}'");
                    return fail(LlErrorKind::Undefined, at, message);
                }
                Ok(MdRef::Node(id))
            }
            Tok::Meta => {
                let kind = MdKind::from_name(&self.text()[1..]);
                self.bump();
                if self.tok.kind != Tok::LParen {
                    return self.syntax("expected '(' here");
                }
                Ok(MdRef::Inline(kind, self.skip_brackets()?))
            }
            _ => self.syntax("expected a metadata node"),
        }
    }

    /// One `!kind node` on a global value, at the `!kind`: `!dbg` is
    /// collected in `self.dbg`, the kinds of `spec/ir.md` §2.2 are
    /// dropped, any other is noted as unsupported.
    pub(crate) fn attachment(&mut self) -> PResult<()> {
        let kind = &self.text()[1..];
        self.bump();
        let node = self.md_ref()?;
        match kind {
            b"dbg" => self.dbg.push(node),
            b"type" | b"vcall_visibility" | b"prof" | b"section_prefix" | b"annotation" => {}
            _ => self.note(format!("global-metadata:{}", String::from_utf8_lossy(kind))),
        }
        Ok(())
    }
}

fn redefined(at: u32, id: u32) -> PResult<()> {
    fail(LlErrorKind::Redefined, at, format!("metadata !{id} is defined twice"))
}
