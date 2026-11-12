//! The kinds of debug node and their typed records (`spec/ir.md` §6.2).
//!
//! One `struct` per kind, with public fields named after LLVM's. A
//! producer fills one in and hands it to [`crate::DebugInfo`]; a consumer
//! gets the same `struct` back, borrowing strings and lists from the
//! table. Stored, a node is a kind, a tag, a mask of the fields that are
//! not at their default, and those fields' cells: a field left at its
//! default costs nothing.
//!
//! The `nodes!` invocation below is the single description of every kind:
//! the Rust field, the LLVM field it is, its type, and the kinds of node
//! it may name. The records, their storage, the by-name setter and the
//! *mapped* rows of the field table (`debug::map`) are all generated from
//! it.

use crate::debug::expr::ExprId;
use crate::debug::names::{
    ChecksumKind, DW_TAG_BASE_TYPE, DW_TAG_TEMPLATE_VALUE_PARAMETER, DiFlags, EmissionKind,
    NameTableKind, SpFlags,
};
use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::{ConstId, DiId};
use crate::strings::{StrId, StrPool};

impl DiId {
    /// "No node": a null element of a node list, such as the `void`
    /// return type that starts a subroutine type's `types`. Single
    /// references are `Option<DiId>` instead.
    pub const NONE: DiId = DiId::new(u32::MAX);

    /// Whether this is [`DiId::NONE`].
    #[inline]
    pub const fn is_none(self) -> bool {
        self.raw() == u32::MAX
    }
}

/// A bound of a subrange: `count`, `lowerBound`, `upperBound`, `stride`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub enum DiBound {
    /// Not given.
    #[default]
    None,
    /// A constant.
    Const(i64),
    /// A variable that holds the bound at run time: the length of a
    /// variable-length array.
    Node(DiId),
    /// An expression that computes it.
    Expr(ExprId),
}

/// A field value as LLVM writes it, for [`DiNode::set`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiFieldValue<'a> {
    /// An unsigned number; also a DWARF constant, a flag set or an
    /// enumeration member, as its number.
    UInt(u64),
    /// A signed number.
    Int(i64),
    /// `true` or `false`.
    Bool(bool),
    /// A string.
    Str(&'a [u8]),
    /// A node reference, or `null`.
    Node(Option<DiId>),
    /// The elements of the tuple a list field names; a `null` element
    /// is [`DiId::NONE`].
    List(&'a [DiId]),
    /// An expression.
    Expr(ExprId),
}

/// Where a node's fields are written.
pub(crate) struct Enc<'t> {
    pub(crate) cells: &'t mut Vec<DiId>,
    pub(crate) strings: &'t mut Option<StrPool>,
    /// The number of strings in `strings` and the bytes they take, kept
    /// here because the pool does not report them.
    pub(crate) string_count: &'t mut u32,
    pub(crate) string_bytes: &'t mut usize,
    /// The number of node ids handed out.
    pub(crate) nodes: u32,
    /// The number of expressions.
    pub(crate) exprs: u32,
}

impl Enc<'_> {
    #[inline]
    fn word(&mut self, word: u32) {
        self.cells.push(DiId::new(word));
    }

    #[inline]
    fn wide(&mut self, value: u64) {
        self.word(value as u32);
        self.word((value >> 32) as u32);
    }

    #[inline]
    fn node(&mut self, id: DiId) -> IrResult<()> {
        if id.raw() >= self.nodes {
            return dangling(id);
        }
        self.cells.push(id);
        Ok(())
    }
}

#[cold]
fn dangling(id: DiId) -> IrResult<()> {
    err(IrErrorKind::UndefinedDebugNode, format!("{id:?} was never reserved or added"))
}

/// Where a node's fields are read from.
pub(crate) struct Dec<'m> {
    pub(crate) cells: &'m [DiId],
    pub(crate) at: usize,
    pub(crate) strings: Option<&'m StrPool>,
}

impl<'m> Dec<'m> {
    #[inline]
    fn word(&mut self) -> u32 {
        let word = self.cells.get(self.at).map_or(0, |cell| cell.raw());
        self.at += 1;
        word
    }

    #[inline]
    fn wide(&mut self) -> u64 {
        let low = u64::from(self.word());
        low | (u64::from(self.word()) << 32)
    }
}

/// A field type: how it is stored, and how it is made from a
/// [`DiFieldValue`].
pub(crate) trait Field<'a>: Copy {
    /// The default: what an omitted field is.
    fn absent() -> Self;
    /// Whether the value differs from the default. Only then is it
    /// stored.
    fn present(&self) -> bool;
    fn put(&self, enc: &mut Enc<'_>) -> IrResult<()>;
    fn get(dec: &mut Dec<'a>) -> Self;
    fn assign(value: DiFieldValue<'a>) -> Option<Self>;
    /// Calls `visit` for every node the value names.
    #[inline]
    fn refs(&self, _visit: &mut dyn FnMut(DiId)) {}
}

macro_rules! number_field {
    ($($ty:ty),*) => {$(
        impl<'a> Field<'a> for $ty {
            #[inline]
            fn absent() -> Self {
                0
            }
            #[inline]
            fn present(&self) -> bool {
                *self != 0
            }
            #[inline]
            fn put(&self, enc: &mut Enc<'_>) -> IrResult<()> {
                enc.word(u32::from(*self));
                Ok(())
            }
            #[inline]
            fn get(dec: &mut Dec<'a>) -> Self {
                // What `put` wrote fits.
                dec.word() as $ty
            }
            fn assign(value: DiFieldValue<'a>) -> Option<Self> {
                match value {
                    DiFieldValue::UInt(value) => <$ty>::try_from(value).ok(),
                    _ => None,
                }
            }
        }
    )*};
}

number_field!(u16, u32);

impl<'a> Field<'a> for u64 {
    #[inline]
    fn absent() -> Self {
        0
    }
    #[inline]
    fn present(&self) -> bool {
        *self != 0
    }
    #[inline]
    fn put(&self, enc: &mut Enc<'_>) -> IrResult<()> {
        enc.wide(*self);
        Ok(())
    }
    #[inline]
    fn get(dec: &mut Dec<'a>) -> Self {
        dec.wide()
    }
    fn assign(value: DiFieldValue<'a>) -> Option<Self> {
        match value {
            DiFieldValue::UInt(value) => Some(value),
            _ => None,
        }
    }
}

impl<'a> Field<'a> for i64 {
    #[inline]
    fn absent() -> Self {
        0
    }
    #[inline]
    fn present(&self) -> bool {
        *self != 0
    }
    #[inline]
    fn put(&self, enc: &mut Enc<'_>) -> IrResult<()> {
        enc.wide(*self as u64);
        Ok(())
    }
    #[inline]
    fn get(dec: &mut Dec<'a>) -> Self {
        dec.wide() as i64
    }
    fn assign(value: DiFieldValue<'a>) -> Option<Self> {
        match value {
            DiFieldValue::Int(value) => Some(value),
            // The bits: an unsigned value past `i64::MAX` reads back
            // through the node's own signedness field.
            DiFieldValue::UInt(value) => Some(value as i64),
            _ => None,
        }
    }
}

impl<'a> Field<'a> for bool {
    #[inline]
    fn absent() -> Self {
        false
    }
    #[inline]
    fn present(&self) -> bool {
        *self
    }
    /// The mask bit is the value.
    #[inline]
    fn put(&self, _enc: &mut Enc<'_>) -> IrResult<()> {
        Ok(())
    }
    #[inline]
    fn get(_dec: &mut Dec<'a>) -> Self {
        true
    }
    fn assign(value: DiFieldValue<'a>) -> Option<Self> {
        match value {
            DiFieldValue::Bool(value) => Some(value),
            _ => None,
        }
    }
}

impl<'a> Field<'a> for Option<u32> {
    #[inline]
    fn absent() -> Self {
        None
    }
    #[inline]
    fn present(&self) -> bool {
        self.is_some()
    }
    #[inline]
    fn put(&self, enc: &mut Enc<'_>) -> IrResult<()> {
        enc.word(self.unwrap_or(0));
        Ok(())
    }
    #[inline]
    fn get(dec: &mut Dec<'a>) -> Self {
        Some(dec.word())
    }
    fn assign(value: DiFieldValue<'a>) -> Option<Self> {
        match value {
            DiFieldValue::UInt(value) => u32::try_from(value).ok().map(Some),
            _ => None,
        }
    }
}

impl<'a> Field<'a> for &'a [u8] {
    #[inline]
    fn absent() -> Self {
        &[]
    }
    #[inline]
    fn present(&self) -> bool {
        !self.is_empty()
    }
    fn put(&self, enc: &mut Enc<'_>) -> IrResult<()> {
        let id = enc.strings.get_or_insert_with(StrPool::default).intern(self)?;
        if id.raw() == *enc.string_count {
            // New: ids are handed out in order.
            *enc.string_count += 1;
            *enc.string_bytes += self.len() + size_of::<(u32, u32)>();
        }
        enc.word(id.raw());
        Ok(())
    }
    #[inline]
    fn get(dec: &mut Dec<'a>) -> Self {
        let raw = dec.word();
        match (dec.strings, StrId::from_raw(raw)) {
            (Some(strings), Some(id)) => strings.get(id),
            _ => &[],
        }
    }
    fn assign(value: DiFieldValue<'a>) -> Option<Self> {
        match value {
            DiFieldValue::Str(text) => Some(text),
            _ => None,
        }
    }
}

impl<'a> Field<'a> for Option<DiId> {
    #[inline]
    fn absent() -> Self {
        None
    }
    #[inline]
    fn present(&self) -> bool {
        self.is_some()
    }
    #[inline]
    fn put(&self, enc: &mut Enc<'_>) -> IrResult<()> {
        enc.node(self.unwrap_or(DiId::NONE))
    }
    #[inline]
    fn get(dec: &mut Dec<'a>) -> Self {
        Some(DiId::new(dec.word()))
    }
    fn assign(value: DiFieldValue<'a>) -> Option<Self> {
        match value {
            DiFieldValue::Node(id) => Some(id),
            _ => None,
        }
    }
    #[inline]
    fn refs(&self, visit: &mut dyn FnMut(DiId)) {
        if let Some(id) = *self {
            visit(id);
        }
    }
}

impl<'a> Field<'a> for &'a [DiId] {
    #[inline]
    fn absent() -> Self {
        &[]
    }
    #[inline]
    fn present(&self) -> bool {
        !self.is_empty()
    }
    fn put(&self, enc: &mut Enc<'_>) -> IrResult<()> {
        let Ok(len) = u32::try_from(self.len()) else {
            return err(IrErrorKind::TableFull, "a debug node list is too long");
        };
        if let Some(&id) = self.iter().find(|id| !id.is_none() && id.raw() >= enc.nodes) {
            return dangling(id);
        }
        enc.word(len);
        enc.cells.extend_from_slice(self);
        Ok(())
    }
    #[inline]
    fn get(dec: &mut Dec<'a>) -> Self {
        let len = dec.word() as usize;
        let list = dec.cells.get(dec.at..).and_then(|rest| rest.get(..len)).unwrap_or(&[]);
        dec.at += len;
        list
    }
    fn assign(value: DiFieldValue<'a>) -> Option<Self> {
        match value {
            DiFieldValue::List(list) => Some(list),
            // `null` where a tuple is expected is the empty list.
            DiFieldValue::Node(None) => Some(&[]),
            _ => None,
        }
    }
    #[inline]
    fn refs(&self, visit: &mut dyn FnMut(DiId)) {
        self.iter().filter(|id| !id.is_none()).for_each(|&id| visit(id));
    }
}

impl<'a> Field<'a> for ExprId {
    #[inline]
    fn absent() -> Self {
        ExprId::EMPTY
    }
    #[inline]
    fn present(&self) -> bool {
        *self != ExprId::EMPTY
    }
    fn put(&self, enc: &mut Enc<'_>) -> IrResult<()> {
        if self.raw() >= enc.exprs {
            return err(IrErrorKind::UndefinedDebugNode, format!("{self:?} does not exist"));
        }
        enc.word(self.raw());
        Ok(())
    }
    #[inline]
    fn get(dec: &mut Dec<'a>) -> Self {
        ExprId::new(dec.word())
    }
    fn assign(value: DiFieldValue<'a>) -> Option<Self> {
        match value {
            DiFieldValue::Expr(id) => Some(id),
            _ => None,
        }
    }
}

impl<'a> Field<'a> for ConstId {
    #[inline]
    fn absent() -> Self {
        ConstId::new(0)
    }
    /// Always stored: constant 0 is a constant like any other.
    #[inline]
    fn present(&self) -> bool {
        true
    }
    #[inline]
    fn put(&self, enc: &mut Enc<'_>) -> IrResult<()> {
        enc.word(self.raw());
        Ok(())
    }
    #[inline]
    fn get(dec: &mut Dec<'a>) -> Self {
        ConstId::new(dec.word())
    }
    fn assign(_value: DiFieldValue<'a>) -> Option<Self> {
        None
    }
}

impl<'a> Field<'a> for DiBound {
    #[inline]
    fn absent() -> Self {
        DiBound::None
    }
    #[inline]
    fn present(&self) -> bool {
        *self != DiBound::None
    }
    fn put(&self, enc: &mut Enc<'_>) -> IrResult<()> {
        match *self {
            DiBound::None | DiBound::Const(_) => {}
            DiBound::Node(id) if id.raw() >= enc.nodes => return dangling(id),
            DiBound::Expr(id) if id.raw() >= enc.exprs => {
                return err(IrErrorKind::UndefinedDebugNode, format!("{id:?} does not exist"));
            }
            DiBound::Node(_) | DiBound::Expr(_) => {}
        }
        let (tag, value) = match *self {
            DiBound::None => (0, 0),
            DiBound::Const(value) => (1, value as u64),
            DiBound::Node(id) => (2, u64::from(id.raw())),
            DiBound::Expr(id) => (3, u64::from(id.raw())),
        };
        enc.word(tag);
        enc.wide(value);
        Ok(())
    }
    fn get(dec: &mut Dec<'a>) -> Self {
        let tag = dec.word();
        let value = dec.wide();
        match tag {
            1 => DiBound::Const(value as i64),
            2 => DiBound::Node(DiId::new(value as u32)),
            3 => DiBound::Expr(ExprId::new(value as u32)),
            _ => DiBound::None,
        }
    }
    fn assign(value: DiFieldValue<'a>) -> Option<Self> {
        match value {
            DiFieldValue::Int(value) => Some(DiBound::Const(value)),
            DiFieldValue::UInt(value) => i64::try_from(value).ok().map(DiBound::Const),
            DiFieldValue::Node(Some(id)) => Some(DiBound::Node(id)),
            DiFieldValue::Node(None) => Some(DiBound::None),
            DiFieldValue::Expr(id) => Some(DiBound::Expr(id)),
            _ => None,
        }
    }
    #[inline]
    fn refs(&self, visit: &mut dyn FnMut(DiId)) {
        if let DiBound::Node(id) = *self {
            visit(id);
        }
    }
}

macro_rules! coded_field {
    ($($ty:ty: $absent:expr, $code:expr, $make:expr;)*) => {$(
        impl<'a> Field<'a> for $ty {
            #[inline]
            fn absent() -> Self {
                $absent
            }
            #[inline]
            fn present(&self) -> bool {
                *self != $absent
            }
            #[inline]
            fn put(&self, enc: &mut Enc<'_>) -> IrResult<()> {
                let code: fn(&$ty) -> u32 = $code;
                enc.word(code(self));
                Ok(())
            }
            #[inline]
            fn get(dec: &mut Dec<'a>) -> Self {
                let make: fn(u64) -> Option<$ty> = $make;
                make(u64::from(dec.word())).unwrap_or($absent)
            }
            fn assign(value: DiFieldValue<'a>) -> Option<Self> {
                let make: fn(u64) -> Option<$ty> = $make;
                match value {
                    DiFieldValue::UInt(value) => make(value),
                    _ => None,
                }
            }
        }
    )*};
}

coded_field! {
    DiFlags: DiFlags(0), |flags| flags.0, |bits| u32::try_from(bits).ok().map(DiFlags);
    SpFlags: SpFlags(0), |flags| flags.0, |bits| u32::try_from(bits).ok().map(SpFlags);
    EmissionKind: EmissionKind::NoDebug, |kind| kind.code(), EmissionKind::from_code;
    NameTableKind: NameTableKind::Default, |kind| kind.code(), NameTableKind::from_code;
    ChecksumKind: ChecksumKind::None, |kind| kind.code(), ChecksumKind::from_code;
}

/// Sets of kinds, one bit per [`DiKind`]: what a reference may name.
pub(crate) mod class {
    use super::DiKind;

    pub(crate) const fn of(kind: DiKind) -> u32 {
        1 << kind as u8
    }

    /// Kinds hod does not know stand for anything LLVM allows there.
    const OTHER: u32 = of(DiKind::Other);
    pub(crate) const ANY: u32 = u32::MAX;
    pub(crate) const FILE: u32 = of(DiKind::File);
    pub(crate) const UNIT: u32 = of(DiKind::CompileUnit);
    pub(crate) const SUBPROGRAM: u32 = of(DiKind::Subprogram);
    pub(crate) const COMPOSITE: u32 = of(DiKind::CompositeType);
    pub(crate) const DERIVED: u32 = of(DiKind::DerivedType);
    pub(crate) const SUBROUTINE: u32 = of(DiKind::SubroutineType);
    pub(crate) const GLOBAL: u32 = of(DiKind::GlobalVariable);
    pub(crate) const GLOBAL_EXPR: u32 = of(DiKind::GlobalVariableExpression);
    pub(crate) const IMPORTED: u32 = of(DiKind::ImportedEntity);
    pub(crate) const VARIABLE: u32 = of(DiKind::LocalVariable) | GLOBAL;
    pub(crate) const TEMPLATE_PARAM: u32 =
        of(DiKind::TemplateTypeParameter) | of(DiKind::TemplateValueParameter);
    /// LLVM's `DIType`.
    pub(crate) const TYPE: u32 =
        of(DiKind::BasicType) | DERIVED | COMPOSITE | SUBROUTINE | OTHER;
    /// LLVM's `DILocalScope`.
    pub(crate) const LOCAL_SCOPE: u32 =
        SUBPROGRAM | of(DiKind::LexicalBlock) | of(DiKind::LexicalBlockFile);
    /// LLVM's `DIScope`.
    pub(crate) const SCOPE: u32 = LOCAL_SCOPE | UNIT | FILE | of(DiKind::Namespace) | TYPE;
}

macro_rules! nodes {
    ($(
        $(#[$meta:meta])*
        $variant:ident = $code:literal, $llvm:literal, $name:ident $(<$lt:lifetime>)? $([tag = $tag:expr])? {
            $( $(#[$fmeta:meta])* $field:ident $fllvm:literal : $fty:ty $(=> $class:expr)? ),* $(,)?
        }
    )*) => {
        /// The kind of a debug node (§6.2).
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        #[repr(u8)]
        pub enum DiKind {
            /// Reserved and not defined yet.
            Undefined = 0,
            $( #[doc = concat!("[`", stringify!($name), "`].")] $variant = $code, )*
        }

        impl DiKind {
            /// Every kind a node can be defined as.
            pub const ALL: &'static [DiKind] = &[$(DiKind::$variant,)*];

            /// LLVM's name for the kind: `DIFile`. The kinds that are
            /// not `!DI...(...)` nodes have a name in parentheses.
            pub const fn llvm_name(self) -> &'static str {
                match self {
                    DiKind::Undefined => "(undefined)",
                    $( DiKind::$variant => $llvm, )*
                }
            }

            /// The kind LLVM writes as `!name(...)`.
            pub fn named(name: &[u8]) -> Option<DiKind> {
                match name {
                    $( _ if name == $llvm.as_bytes() => Some(DiKind::$variant), )*
                    _ => None,
                }
            }

            /// LLVM's names of the fields the kind's record has, in the
            /// order of the record: the *mapped* fields of §6.9.
            pub const fn fields(self) -> &'static [&'static str] {
                match self {
                    DiKind::Undefined => &[],
                    $( DiKind::$variant => &[$(nodes!(@second $tag, "tag"),)? $($fllvm,)*], )*
                }
            }

            pub(crate) const fn from_code(code: u8) -> DiKind {
                match code {
                    $( $code => DiKind::$variant, )*
                    _ => DiKind::Undefined,
                }
            }
        }

        $(
            $(#[$meta])*
            #[doc = concat!("\n\nLLVM's `", $llvm, "`.")]
            #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
            pub struct $name $(<$lt>)? {
                $(
                    #[doc = concat!("`tag:`, a `DW_TAG_*` number; `", stringify!($tag), "` by default.")]
                    pub tag: u16,
                )?
                $( $(#[$fmeta])* #[doc = concat!("\n\n`", $fllvm, ":`.")] pub $field: $fty, )*
            }

            impl $(<$lt>)? Default for $name $(<$lt>)? {
                fn default() -> Self {
                    Self { $( tag: $tag, )? $( $field: <$fty as Field<'_>>::absent(), )* }
                }
            }

            impl<'a> From<$name $(<$lt>)?> for DiNode<'a> {
                fn from(node: $name $(<$lt>)?) -> DiNode<'a> {
                    DiNode::$variant(node)
                }
            }

            impl<'a> $name $(<$lt>)? {
                /// Writes the fields that are not at their default;
                /// returns the tag and the mask of those fields.
                #[allow(unused_assignments, reason = "the last field's bit is not read")]
                fn encode(&self, enc: &mut Enc<'_>) -> IrResult<(u16, u32)> {
                    let mut mask = 0u32;
                    let mut bit = 1u32;
                    $(
                        if Field::present(&self.$field) {
                            mask |= bit;
                            Field::put(&self.$field, enc)?;
                        }
                        bit <<= 1;
                    )*
                    Ok((nodes!(@tag_or [$($tag)?] self.tag, 0), mask))
                }

                #[allow(unused_assignments, unused_variables, reason = "the last field's bit is not read; not every kind has a tag")]
                fn decode(tag: u16, mask: u32, dec: &mut Dec<'a>) -> Self {
                    let mut bit = 1u32;
                    Self {
                        $( tag: nodes!(@second $tag, tag), )?
                        $( $field: {
                            let present = mask & bit != 0;
                            bit <<= 1;
                            if present { Field::get(dec) } else { Field::absent() }
                        }, )*
                    }
                }

                /// `None` for a field the record does not have,
                /// `Some(false)` for a value of the wrong shape.
                fn set(&mut self, field: &[u8], value: DiFieldValue<'a>) -> Option<bool> {
                    $(
                        if field == b"tag" {
                            let tag = <u16 as Field<'_>>::assign(value);
                            self.tag = tag.unwrap_or(nodes!(@second $tag, self.tag));
                            return Some(tag.is_some());
                        }
                    )?
                    $(
                        if field == $fllvm.as_bytes() {
                            let value = Field::assign(value);
                            if let Some(value) = value {
                                self.$field = value;
                            }
                            return Some(value.is_some());
                        }
                    )*
                    None
                }

                fn refs(&self, visit: &mut dyn FnMut(&'static str, u32, DiId)) {
                    $(
                        Field::refs(&self.$field, &mut |id| {
                            visit($fllvm, nodes!(@class $($class)?), id);
                        });
                    )*
                }
            }
        )*

        /// A debug node (§6.2): what a producer defines and a consumer
        /// reads.
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum DiNode<'a> {
            /// Reserved and not defined yet. Cannot be defined as this.
            Undefined,
            $( #[doc = concat!("LLVM's `", $llvm, "`.")] $variant($name $(<$lt>)?), )*
        }

        impl<'a> DiNode<'a> {
            /// The node's kind.
            pub const fn kind(&self) -> DiKind {
                match self {
                    DiNode::Undefined => DiKind::Undefined,
                    $( DiNode::$variant(_) => DiKind::$variant, )*
                }
            }

            /// A node of `kind` with every field at its default.
            pub fn new(kind: DiKind) -> DiNode<'a> {
                match kind {
                    DiKind::Undefined => DiNode::Undefined,
                    $( DiKind::$variant => DiNode::$variant(<$name>::default()), )*
                }
            }

            pub(crate) fn encode(&self, enc: &mut Enc<'_>) -> IrResult<(u16, u32)> {
                match self {
                    DiNode::Undefined => err(
                        IrErrorKind::InvalidDebugInfo,
                        "a node cannot be defined as undefined",
                    ),
                    $( DiNode::$variant(node) => node.encode(enc), )*
                }
            }

            pub(crate) fn decode(kind: DiKind, tag: u16, mask: u32, dec: &mut Dec<'a>) -> DiNode<'a> {
                match kind {
                    DiKind::Undefined => DiNode::Undefined,
                    $( DiKind::$variant => DiNode::$variant(<$name>::decode(tag, mask, dec)), )*
                }
            }

            fn set_field(&mut self, field: &[u8], value: DiFieldValue<'a>) -> Option<bool> {
                match self {
                    DiNode::Undefined => None,
                    $( DiNode::$variant(node) => node.set(field, value), )*
                }
            }

            /// Calls `visit(field, class, id)` for every node this one
            /// names: LLVM's field name, the kinds the field may name (a
            /// `class` bit set) and the id.
            pub(crate) fn refs(&self, visit: &mut dyn FnMut(&'static str, u32, DiId)) {
                match self {
                    DiNode::Undefined => {}
                    $( DiNode::$variant(node) => node.refs(visit), )*
                }
            }
        }
    };
    (@second $first:expr, $second:expr) => { $second };
    (@tag_or [] $with:expr, $without:expr) => { $without };
    (@tag_or [$tag:expr] $with:expr, $without:expr) => { $with };
    (@class) => { class::ANY };
    (@class $class:expr) => { $class };
}

nodes! {
    /// A translation unit. Always distinct. Listed in
    /// [`crate::DebugInfo::compile_units`] when it is in `!llvm.dbg.cu`.
    CompileUnit = 1, "DICompileUnit", DiCompileUnit<'a> {
        /// A `DW_LANG_*` number.
        language "language": u16,
        /// The primary source file.
        file "file": Option<DiId> => class::FILE,
        /// The producer string, `DW_AT_producer`.
        producer "producer": &'a [u8],
        /// Whether the unit was optimized (`DW_AT_APPLE_optimized`).
        is_optimized "isOptimized": bool,
        /// The command line (`DW_AT_APPLE_flags`).
        flags "flags": &'a [u8],
        /// The Objective-C runtime version.
        runtime_version "runtimeVersion": u32,
        /// How much to emit for the unit.
        emission_kind "emissionKind": EmissionKind,
        /// The enumeration types of the unit.
        enums "enums": &'a [DiId] => class::COMPOSITE,
        /// Types to emit even if nothing refers to them, and subprogram
        /// declarations likewise.
        retained_types "retainedTypes": &'a [DiId] => class::TYPE | class::SUBPROGRAM,
        /// The global-variable expressions of the unit.
        globals "globals": &'a [DiId] => class::GLOBAL_EXPR,
        /// The unit's imported entities (`using` at namespace scope).
        imports "imports": &'a [DiId] => class::IMPORTED,
        /// Which name index to emit.
        name_table_kind "nameTableKind": NameTableKind,
        /// `DW_AT_LLVM_sysroot`.
        sysroot "sysroot": &'a [u8],
        /// `DW_AT_APPLE_sdk`.
        sdk "sdk": &'a [u8],
    }

    /// A source file.
    File = 2, "DIFile", DiFile<'a> {
        /// The name, relative to `directory` unless absolute.
        filename "filename": &'a [u8],
        /// The directory.
        directory "directory": &'a [u8],
        /// The kind of `checksum`.
        checksum_kind "checksumkind": ChecksumKind,
        /// The checksum as hexadecimal digits.
        checksum "checksum": &'a [u8],
        /// The source text, when it is embedded. An embedded empty text
        /// reads as none.
        source "source": &'a [u8],
    }

    /// A function: a definition (distinct, with `unit`) or a
    /// declaration (a member function in its class, or what a
    /// definition's `declaration` names).
    Subprogram = 3, "DISubprogram", DiSubprogram<'a> {
        /// The source name.
        name "name": &'a [u8],
        /// The mangled name.
        linkage_name "linkageName": &'a [u8],
        /// The enclosing scope.
        scope "scope": Option<DiId> => class::SCOPE,
        /// The file of the declaration.
        file "file": Option<DiId> => class::FILE,
        /// The line of the declaration.
        line "line": u32,
        /// The subroutine type.
        ty "type": Option<DiId> => class::SUBROUTINE,
        /// The line of the opening brace.
        scope_line "scopeLine": u32,
        /// The class whose vtable has the function.
        containing_type "containingType": Option<DiId> => class::TYPE,
        /// The vtable slot of a virtual function.
        virtual_index "virtualIndex": u32,
        /// `DIFlag*` bits.
        flags "flags": DiFlags,
        /// `DISPFlag*` bits.
        sp_flags "spFlags": SpFlags,
        /// The compile unit of a definition.
        unit "unit": Option<DiId> => class::UNIT,
        /// The template parameters.
        template_params "templateParams": &'a [DiId] => class::TEMPLATE_PARAM,
        /// The declaration this defines.
        declaration "declaration": Option<DiId> => class::SUBPROGRAM,
        /// Local variables, labels and imported entities to emit even
        /// when no record names them.
        retained_nodes "retainedNodes": &'a [DiId],
        /// The types the function may throw.
        thrown_types "thrownTypes": &'a [DiId] => class::TYPE,
    }

    /// A nested block of a function.
    LexicalBlock = 4, "DILexicalBlock", DiLexicalBlock {
        /// The enclosing block or subprogram.
        scope "scope": Option<DiId> => class::LOCAL_SCOPE,
        /// The file.
        file "file": Option<DiId> => class::FILE,
        /// The line it starts at.
        line "line": u32,
        /// The column it starts at.
        column "column": u16,
    }

    /// A part of a scope that comes from another file (an `#include`
    /// inside a function), or one basic block of several on a line.
    LexicalBlockFile = 5, "DILexicalBlockFile", DiLexicalBlockFile {
        /// The enclosing block or subprogram.
        scope "scope": Option<DiId> => class::LOCAL_SCOPE,
        /// The file.
        file "file": Option<DiId> => class::FILE,
        /// The line-table discriminator.
        discriminator "discriminator": u32,
    }

    /// A namespace. Anonymous when it has no name.
    Namespace = 6, "DINamespace", DiNamespace<'a> {
        /// The name.
        name "name": &'a [u8],
        /// The enclosing scope.
        scope "scope": Option<DiId> => class::SCOPE,
        /// An inline namespace.
        export_symbols "exportSymbols": bool,
    }

    /// A local variable or a parameter.
    LocalVariable = 7, "DILocalVariable", DiLocalVariable<'a> {
        /// The name.
        name "name": &'a [u8],
        /// For a parameter, its one-based position; 0 for a local.
        arg "arg": u16,
        /// The block or subprogram it is declared in.
        scope "scope": Option<DiId> => class::LOCAL_SCOPE,
        /// The file.
        file "file": Option<DiId> => class::FILE,
        /// The line.
        line "line": u32,
        /// The type.
        ty "type": Option<DiId> => class::TYPE,
        /// `DIFlag*` bits.
        flags "flags": DiFlags,
        /// The alignment in bits when the source asks for one.
        align "align": u32,
    }

    /// A global variable, a static member's definition or a constant.
    GlobalVariable = 8, "DIGlobalVariable", DiGlobalVariable<'a> {
        /// The source name.
        name "name": &'a [u8],
        /// The mangled name.
        linkage_name "linkageName": &'a [u8],
        /// The enclosing scope.
        scope "scope": Option<DiId> => class::SCOPE,
        /// The file.
        file "file": Option<DiId> => class::FILE,
        /// The line.
        line "line": u32,
        /// The type.
        ty "type": Option<DiId> => class::TYPE,
        /// Internal linkage.
        is_local "isLocal": bool,
        /// A definition. LLVM assumes `true` when the field is omitted;
        /// [`DiNode::llvm_default`] starts from that.
        is_definition "isDefinition": bool,
        /// The template parameters of a variable template.
        template_params "templateParams": &'a [DiId] => class::TEMPLATE_PARAM,
        /// The static member this defines.
        declaration "declaration": Option<DiId> => class::DERIVED,
        /// The alignment in bits when the source asks for one.
        align "align": u32,
    }

    /// A global variable with the expression that locates it, or that
    /// gives its value when it has no storage. What a variable symbol
    /// and a compile unit's `globals` name.
    GlobalVariableExpression = 9, "DIGlobalVariableExpression", DiGlobalVariableExpression {
        /// The variable.
        var "var": Option<DiId> => class::GLOBAL,
        /// The expression.
        expr "expr": ExprId,
    }

    /// A label.
    Label = 10, "DILabel", DiLabel<'a> {
        /// The block or subprogram it is in.
        scope "scope": Option<DiId> => class::LOCAL_SCOPE,
        /// The name.
        name "name": &'a [u8],
        /// The file.
        file "file": Option<DiId> => class::FILE,
        /// The line.
        line "line": u32,
        /// The column (LLVM 21 and later).
        column "column": u32,
        /// A label the compiler made up (LLVM 21 and later).
        is_artificial "isArtificial": bool,
        /// The suspend point of a coroutine the label marks (LLVM 21
        /// and later).
        coro_suspend_idx "coroSuspendIdx": Option<u32>,
    }

    /// A `using` declaration or directive, or a namespace alias.
    ImportedEntity = 11, "DIImportedEntity", DiImportedEntity<'a> [tag = 0] {
        /// The name an alias introduces.
        name "name": &'a [u8],
        /// Where the `using` is.
        scope "scope": Option<DiId> => class::SCOPE,
        /// What is imported.
        entity "entity": Option<DiId>,
        /// The file.
        file "file": Option<DiId> => class::FILE,
        /// The line.
        line "line": u32,
        /// The renamed entities of a Fortran `use`.
        elements "elements": &'a [DiId],
    }

    /// A base type, or the unspecified type (`decltype(nullptr)`).
    BasicType = 12, "DIBasicType", DiBasicType<'a> [tag = DW_TAG_BASE_TYPE] {
        /// The name.
        name "name": &'a [u8],
        /// The size in bits.
        size "size": u64,
        /// The alignment in bits.
        align "align": u32,
        /// A `DW_ATE_*` number.
        encoding "encoding": u16,
        /// `DIFlag*` bits: the endianity.
        flags "flags": DiFlags,
        /// The number of bits that hold the value, when fewer than
        /// `size` (LLVM 21 and later).
        data_size "dataSize": u32,
    }

    /// A type made from one other: a pointer, reference, qualifier or
    /// typedef; a member of a class; a base class.
    DerivedType = 13, "DIDerivedType", DiDerivedType<'a> [tag = 0] {
        /// The name.
        name "name": &'a [u8],
        /// The enclosing scope: of a member, its class.
        scope "scope": Option<DiId> => class::SCOPE,
        /// The file.
        file "file": Option<DiId> => class::FILE,
        /// The line.
        line "line": u32,
        /// The type this is made from; none is `void`.
        base_type "baseType": Option<DiId> => class::TYPE,
        /// The size in bits.
        size "size": u64,
        /// The alignment in bits.
        align "align": u32,
        /// The offset in bits of a member or base class.
        offset "offset": u64,
        /// `DIFlag*` bits.
        flags "flags": DiFlags,
        /// By tag: the class of a pointer to member; the storage offset
        /// of a bit-field or the value of a static constant member (a
        /// [`DiConstant`]); the virtual-base offset of an inheritance.
        extra_data "extraData": Option<DiId>,
        /// The DWARF address space of a pointer or reference.
        dwarf_address_space "dwarfAddressSpace": Option<u32>,
    }

    /// A class, struct, union, enumeration or array. One that is a
    /// forward declaration with an `identifier` can be completed
    /// ([`crate::DebugInfo::complete`]).
    CompositeType = 14, "DICompositeType", DiCompositeType<'a> [tag = 0] {
        /// The name.
        name "name": &'a [u8],
        /// The enclosing scope.
        scope "scope": Option<DiId> => class::SCOPE,
        /// The file.
        file "file": Option<DiId> => class::FILE,
        /// The line.
        line "line": u32,
        /// The element type of an array; the underlying type of an
        /// enumeration.
        base_type "baseType": Option<DiId> => class::TYPE,
        /// The size in bits.
        size "size": u64,
        /// The alignment in bits.
        align "align": u32,
        /// The offset in bits.
        offset "offset": u64,
        /// `DIFlag*` bits.
        flags "flags": DiFlags,
        /// Members, base classes, member functions, enumerators, or an
        /// array's subranges.
        elements "elements": &'a [DiId],
        /// The Objective-C runtime language.
        runtime_lang "runtimeLang": u16,
        /// The class that holds the vtable pointer.
        vtable_holder "vtableHolder": Option<DiId> => class::TYPE,
        /// The template parameters.
        template_params "templateParams": &'a [DiId] => class::TEMPLATE_PARAM,
        /// The mangled name of the type, by which definitions in
        /// different units are the same type.
        identifier "identifier": &'a [u8],
    }

    /// A function type.
    SubroutineType = 15, "DISubroutineType", DiSubroutineType<'a> {
        /// `DIFlag*` bits: the reference qualifier.
        flags "flags": DiFlags,
        /// A `DW_CC_*` number.
        cc "cc": u16,
        /// The return type, then the parameter types. A null return
        /// type is `void`; a null last entry is `...`.
        types "types": &'a [DiId] => class::TYPE,
    }

    /// One dimension of an array.
    Subrange = 16, "DISubrange", DiSubrange {
        /// The number of elements; -1 for an unknown bound.
        count "count": DiBound,
        /// The lower bound.
        lower_bound "lowerBound": DiBound,
        /// The upper bound.
        upper_bound "upperBound": DiBound,
        /// The stride.
        stride "stride": DiBound,
    }

    /// One enumerator.
    Enumerator = 17, "DIEnumerator", DiEnumerator<'a> {
        /// The name.
        name "name": &'a [u8],
        /// The value's low 64 bits; signed or not by `is_unsigned`. A
        /// wider value is the unsupported construct
        /// `debug:DIEnumerator.wide`.
        value "value": i64,
        /// Whether `value` is unsigned.
        is_unsigned "isUnsigned": bool,
    }

    /// A template type parameter with its argument.
    TemplateTypeParameter = 18, "DITemplateTypeParameter", DiTemplateTypeParameter<'a> {
        /// The parameter's name.
        name "name": &'a [u8],
        /// The argument.
        ty "type": Option<DiId> => class::TYPE,
        /// The argument is the parameter's default.
        defaulted "defaulted": bool,
    }

    /// A template non-type parameter with its argument; with other tags,
    /// a template template parameter or a parameter pack.
    TemplateValueParameter = 19, "DITemplateValueParameter", DiTemplateValueParameter<'a> [tag = DW_TAG_TEMPLATE_VALUE_PARAMETER] {
        /// The parameter's name.
        name "name": &'a [u8],
        /// The parameter's type.
        ty "type": Option<DiId> => class::TYPE,
        /// The argument is the parameter's default.
        defaulted "defaulted": bool,
        /// The argument: a [`DiConstant`]; for a template template
        /// parameter a [`DiString`] with the template's name; for a
        /// pack a [`DiTuple`] of parameters.
        value "value": Option<DiId>,
    }

    /// A constant used as metadata: `i32 9` where a node is expected.
    Constant = 20, "(constant)", DiConstant {
        /// The constant, in the module's pool.
        value "value": ConstId,
    }

    /// A string used as metadata: `!"text"` where a node is expected.
    String = 21, "(string)", DiString<'a> {
        /// The text.
        text "text": &'a [u8],
    }

    /// A tuple used as a value: `!{...}` where a single node is
    /// expected. The tuple a list field names is not a node: its
    /// elements are the field.
    Tuple = 22, "(tuple)", DiTuple<'a> {
        /// The elements; a null element is [`DiId::NONE`].
        elements "elements": &'a [DiId],
    }

    /// A node of a kind hod has no record for. It keeps the graph whole
    /// and its kind's name; `classify` refuses the module as
    /// `debug:<kind>`.
    Other = 23, "(other)", DiOther<'a> {
        /// LLVM's name for the kind: `DIStringType`.
        kind "kind": &'a [u8],
    }
}

impl<'a> DiNode<'a> {
    /// A node of `kind` as LLVM's parser starts one: every field at
    /// LLVM's default for an omitted field. That differs from
    /// [`DiNode::new`] in one place: a global variable is a definition.
    pub fn llvm_default(kind: DiKind) -> DiNode<'a> {
        match kind {
            DiKind::GlobalVariable => {
                DiNode::GlobalVariable(DiGlobalVariable { is_definition: true, ..Default::default() })
            }
            other => DiNode::new(other),
        }
    }

    /// Sets the field LLVM calls `field` to `value`: the way a producer
    /// that holds LLVM's names (a parser of the text, a frontend's
    /// name-and-field model) fills a node. `field` is one of
    /// [`DiKind::fields`], or one of the older spellings of a
    /// subprogram's flags (`isLocal`, `isDefinition`, `isOptimized`,
    /// `virtuality`), which set bits of `spFlags`.
    ///
    /// A producer looks the field up in the field table first
    /// ([`crate::di_field`]): only a *mapped* field is set.
    ///
    /// # Errors
    /// `InvalidDebugInfo` if the kind has no such field, or the value is
    /// not of the field's shape or does not fit it.
    pub fn set(&mut self, field: &[u8], value: DiFieldValue<'a>) -> IrResult<()> {
        if let DiNode::Subprogram(node) = self {
            let bits = match (field, value) {
                (b"isLocal", DiFieldValue::Bool(on)) => Some((SpFlags::LOCAL_TO_UNIT.0, on)),
                (b"isDefinition", DiFieldValue::Bool(on)) => Some((SpFlags::DEFINITION.0, on)),
                (b"isOptimized", DiFieldValue::Bool(on)) => Some((SpFlags::OPTIMIZED.0, on)),
                (b"virtuality", DiFieldValue::UInt(which @ 0..=2)) => {
                    node.sp_flags.0 &= !3;
                    Some((which as u32, true))
                }
                _ => None,
            };
            if let Some((bits, on)) = bits {
                node.sp_flags.0 = if on { node.sp_flags.0 | bits } else { node.sp_flags.0 & !bits };
                return Ok(());
            }
        }
        let what = match self.set_field(field, value) {
            Some(true) => return Ok(()),
            Some(false) => "cannot be",
            None => "is not a field hod_ir has; it was given",
        };
        err(
            IrErrorKind::InvalidDebugInfo,
            format!(
                "{}.{} {what} {value:?}",
                self.kind().llvm_name(),
                String::from_utf8_lossy(field)
            ),
        )
    }

    /// Whether LLVM requires the node to be `distinct`: a compile unit,
    /// and a subprogram that is a definition.
    pub fn must_be_distinct(&self) -> bool {
        match self {
            DiNode::CompileUnit(_) => true,
            DiNode::Subprogram(node) => node.sp_flags.contains(SpFlags::DEFINITION),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §6.2: kinds have stable codes, LLVM names and field lists.
    #[test]
    fn kinds_name_themselves() {
        assert_eq!(DiKind::ALL.len(), 23);
        for (index, &kind) in DiKind::ALL.iter().enumerate() {
            assert_eq!(kind as usize, index + 1);
            assert_eq!(DiKind::from_code(kind as u8), kind);
            assert_eq!(DiNode::new(kind).kind(), kind);
            let named = DiKind::named(kind.llvm_name().as_bytes());
            assert_eq!(named, Some(kind));
            assert!(kind.fields().len() <= 32, "the mask is 32 bits");
        }
        assert_eq!(DiKind::named(b"DIStringType"), None);
        assert_eq!(DiKind::from_code(99), DiKind::Undefined);
        assert_eq!(DiKind::File.fields(), ["filename", "directory", "checksumkind", "checksum", "source"]);
        assert_eq!(DiKind::BasicType.fields()[0], "tag");
        assert_eq!(DiKind::Subprogram.fields().len(), 16);
    }

    /// §6.2: defaults are LLVM's, with the one stated exception.
    #[test]
    fn defaults() {
        assert_eq!(DiBasicType::default().tag, DW_TAG_BASE_TYPE);
        assert_eq!(DiTemplateValueParameter::default().tag, DW_TAG_TEMPLATE_VALUE_PARAMETER);
        assert_eq!(DiDerivedType::default().tag, 0);
        let DiNode::GlobalVariable(plain) = DiNode::new(DiKind::GlobalVariable) else { panic!() };
        let DiNode::GlobalVariable(llvm) = DiNode::llvm_default(DiKind::GlobalVariable) else {
            panic!()
        };
        assert!(!plain.is_definition && llvm.is_definition);
        assert_eq!(DiNode::llvm_default(DiKind::File), DiNode::File(DiFile::default()));
        assert!(DiNode::new(DiKind::CompileUnit).must_be_distinct());
        assert!(!DiNode::new(DiKind::Subprogram).must_be_distinct());
    }

    /// §6.9: `set` fills a record by LLVM's field names and refuses what
    /// does not fit.
    #[test]
    fn set_by_llvm_name() {
        let mut node = DiNode::new(DiKind::Subprogram);
        node.set(b"name", DiFieldValue::Str(b"f")).unwrap();
        node.set(b"line", DiFieldValue::UInt(7)).unwrap();
        node.set(b"scope", DiFieldValue::Node(Some(DiId::new(3)))).unwrap();
        node.set(b"spFlags", DiFieldValue::UInt(u64::from(SpFlags::DEFINITION.0))).unwrap();
        node.set(b"isLocal", DiFieldValue::Bool(true)).unwrap();
        node.set(b"virtuality", DiFieldValue::UInt(2)).unwrap();
        node.set(b"isOptimized", DiFieldValue::Bool(false)).unwrap();
        node.set(b"retainedNodes", DiFieldValue::Node(None)).unwrap();
        let DiNode::Subprogram(sp) = node else { panic!() };
        assert_eq!((sp.name, sp.line, sp.scope), (&b"f"[..], 7, Some(DiId::new(3))));
        assert_eq!(sp.sp_flags, SpFlags::DEFINITION | SpFlags::LOCAL_TO_UNIT | SpFlags::PURE_VIRTUAL);
        assert!(node.must_be_distinct());

        let kind = |result: IrResult<()>| result.unwrap_err().kind;
        assert_eq!(kind(node.set(b"line", DiFieldValue::Str(b"7"))), IrErrorKind::InvalidDebugInfo);
        assert_eq!(kind(node.set(b"line", DiFieldValue::UInt(1 << 32))), IrErrorKind::InvalidDebugInfo);
        assert_eq!(kind(node.set(b"annotations", DiFieldValue::Node(None))), IrErrorKind::InvalidDebugInfo);
        assert_eq!(kind(node.set(b"virtuality", DiFieldValue::UInt(3))), IrErrorKind::InvalidDebugInfo);

        let mut ty = DiNode::new(DiKind::DerivedType);
        ty.set(b"tag", DiFieldValue::UInt(0x0f)).unwrap();
        ty.set(b"size", DiFieldValue::UInt(1 << 40)).unwrap();
        ty.set(b"dwarfAddressSpace", DiFieldValue::UInt(0)).unwrap();
        assert_eq!(kind(ty.set(b"tag", DiFieldValue::UInt(1 << 16))), IrErrorKind::InvalidDebugInfo);
        let DiNode::DerivedType(ty) = ty else { panic!() };
        assert_eq!((ty.tag, ty.size, ty.dwarf_address_space), (0x0f, 1 << 40, Some(0)));

        let mut range = DiNode::new(DiKind::Subrange);
        range.set(b"count", DiFieldValue::Int(-1)).unwrap();
        range.set(b"lowerBound", DiFieldValue::Node(Some(DiId::new(4)))).unwrap();
        range.set(b"upperBound", DiFieldValue::Expr(ExprId::new(2))).unwrap();
        let DiNode::Subrange(range) = range else { panic!() };
        assert_eq!(range.count, DiBound::Const(-1));
        assert_eq!(range.lower_bound, DiBound::Node(DiId::new(4)));
        assert_eq!(range.upper_bound, DiBound::Expr(ExprId::new(2)));

        let mut unit = DiNode::new(DiKind::CompileUnit);
        unit.set(b"emissionKind", DiFieldValue::UInt(2)).unwrap();
        assert_eq!(kind(unit.set(b"emissionKind", DiFieldValue::UInt(9))), IrErrorKind::InvalidDebugInfo);
        let mut value = DiNode::new(DiKind::Enumerator);
        value.set(b"value", DiFieldValue::UInt(u64::MAX)).unwrap();
        assert_eq!(value, DiNode::Enumerator(DiEnumerator { value: -1, ..Default::default() }));
        assert_eq!(kind(DiNode::Undefined.set(b"x", DiFieldValue::Bool(true))), IrErrorKind::InvalidDebugInfo);
    }
}
