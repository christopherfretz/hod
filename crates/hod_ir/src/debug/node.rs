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
use crate::intern::{Layering, NoLayer};
use crate::strings::{StrId, StrPool};
use std::marker::PhantomData;

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

/// What kind of value a field holds (§6.2): what a producer that reads
/// LLVM's text must make of the value written there.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DiShape {
    /// An unsigned number ([`DiFieldValue::UInt`]): a count, a size, a
    /// line, or a DWARF constant (`tag`, `encoding`, `language`, `cc`).
    UInt,
    /// A signed number ([`DiFieldValue::Int`]).
    Int,
    /// A number that may be absent, distinct from 0.
    OptUInt,
    /// `true` or `false`.
    Bool,
    /// A string.
    Str,
    /// A single node, or `null`.
    Node,
    /// The elements of a tuple.
    List,
    /// An expression.
    Expr,
    /// A constant of the module's pool.
    Const,
    /// A subrange bound: a number, a node or an expression.
    Bound,
    /// `DIFlag*` bits.
    Flags,
    /// `DISPFlag*` bits.
    SpFlags,
    /// An [`EmissionKind`].
    EmissionKind,
    /// A [`NameTableKind`].
    NameTableKind,
    /// A [`ChecksumKind`].
    ChecksumKind,
}

/// A field that is not at its default, as [`DiNode::each_field`] hands
/// it over: the value in the field's own type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiFieldOut<'a> {
    /// An unsigned number; `tag` and the other DWARF constants too.
    UInt(u64),
    /// A signed number.
    Int(i64),
    /// `true` (a boolean field at its default is not handed over).
    Bool(bool),
    /// A string.
    Str(&'a [u8]),
    /// A node.
    Node(DiId),
    /// A list of nodes; a null element is [`DiId::NONE`].
    List(&'a [DiId]),
    /// An expression.
    Expr(ExprId),
    /// A constant.
    Const(ConstId),
    /// A subrange bound.
    Bound(DiBound),
    /// `DIFlag*` bits.
    Flags(DiFlags),
    /// `DISPFlag*` bits.
    SpFlags(SpFlags),
    /// An emission kind.
    EmissionKind(EmissionKind),
    /// A name-table kind.
    NameTableKind(NameTableKind),
    /// A checksum kind.
    ChecksumKind(ChecksumKind),
}

/// Where a node's fields are written.
pub(crate) struct Enc<'t, L: Layering> {
    pub(crate) cells: &'t mut crate::intern::Seg<DiId, L>,
    pub(crate) strings: &'t mut Option<Box<StrPool<L>>>,
    /// The number of strings in `strings` and the bytes they take, kept
    /// here because the pool does not report them.
    pub(crate) string_count: &'t mut u32,
    pub(crate) string_bytes: &'t mut usize,
    /// The number of node ids handed out.
    pub(crate) nodes: u32,
    /// The number of expressions.
    pub(crate) exprs: u32,
}

impl<L: Layering> Enc<'_, L> {
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

/// Where a node's fields are read from: the words after the record's
/// mask, consumed from the front. Past the end every word reads as 0.
pub(crate) struct Dec<'m, L: Layering = NoLayer> {
    pub(crate) cells: &'m [DiId],
    pub(crate) strings: Option<&'m StrPool<L>>,
}

impl<L: Layering> Dec<'_, L> {
    #[inline]
    fn word(&mut self) -> u32 {
        match self.cells.split_first() {
            Some((cell, rest)) => {
                self.cells = rest;
                cell.raw()
            }
            None => 0,
        }
    }

    #[inline]
    fn wide(&mut self) -> u64 {
        let low = u64::from(self.word());
        low | (u64::from(self.word()) << 32)
    }
}

/// [`Field::CELLS`] of a list: its length cell, then that many cells.
pub(crate) const LIST_CELLS: u8 = u8::MAX;

/// A field type: how it is stored, and how it is made from a
/// [`DiFieldValue`].
pub(crate) trait Field<'a>: Copy {
    /// The default: what an omitted field is.
    fn absent() -> Self;
    /// Whether the value differs from the default. Only then is it
    /// stored.
    fn present(&self) -> bool;
    fn put<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<()>;
    fn get<L: Layering>(dec: &mut Dec<'a, L>) -> Self;
    fn assign(value: DiFieldValue<'a>) -> Option<Self>;
    /// What kind of value the field holds.
    const SHAPE: DiShape;
    /// The cells a stored value takes; [`LIST_CELLS`] for a list (its
    /// length, then that many cells).
    const CELLS: u8 = 1;
    /// The value, for a field that is [`Field::present`].
    fn out(&self) -> DiFieldOut<'a>;
    /// Calls `visit` for every node the value names.
    #[inline]
    fn refs(&self, _visit: &mut impl FnMut(DiId)) {}
    /// Consumes the stored value from `dec` as [`Field::get`] does and
    /// calls `visit` with the number of cells left in `dec` at each cell
    /// that holds a node id (the uniquing pass, §6.3, rewrites them).
    #[inline]
    fn ref_cells<L: Layering>(dec: &mut Dec<'a, L>, _visit: &mut dyn FnMut(usize)) {
        let _ = Self::get(dec);
    }
}

macro_rules! number_field {
    ($($ty:ty),*) => {$(
        impl<'a> Field<'a> for $ty {
            const SHAPE: DiShape = DiShape::UInt;
            #[inline]
            fn out(&self) -> DiFieldOut<'a> {
                DiFieldOut::UInt(u64::from(*self))
            }
            #[inline]
            fn absent() -> Self {
                0
            }
            #[inline]
            fn present(&self) -> bool {
                *self != 0
            }
            #[inline]
            fn put<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<()> {
                enc.word(u32::from(*self));
                Ok(())
            }
            #[inline]
            fn get<L: Layering>(dec: &mut Dec<'a, L>) -> Self {
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
    const SHAPE: DiShape = DiShape::UInt;
    const CELLS: u8 = 2;
    #[inline]
    fn out(&self) -> DiFieldOut<'a> {
        DiFieldOut::UInt(*self)
    }
    #[inline]
    fn absent() -> Self {
        0
    }
    #[inline]
    fn present(&self) -> bool {
        *self != 0
    }
    #[inline]
    fn put<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<()> {
        enc.wide(*self);
        Ok(())
    }
    #[inline]
    fn get<L: Layering>(dec: &mut Dec<'a, L>) -> Self {
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
    const SHAPE: DiShape = DiShape::Int;
    const CELLS: u8 = 2;
    #[inline]
    fn out(&self) -> DiFieldOut<'a> {
        DiFieldOut::Int(*self)
    }
    #[inline]
    fn absent() -> Self {
        0
    }
    #[inline]
    fn present(&self) -> bool {
        *self != 0
    }
    #[inline]
    fn put<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<()> {
        enc.wide(*self as u64);
        Ok(())
    }
    #[inline]
    fn get<L: Layering>(dec: &mut Dec<'a, L>) -> Self {
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
    const SHAPE: DiShape = DiShape::Bool;
    /// A `bool` is its bit in the mask.
    const CELLS: u8 = 0;
    #[inline]
    fn out(&self) -> DiFieldOut<'a> {
        DiFieldOut::Bool(*self)
    }
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
    fn put<L: Layering>(&self, _enc: &mut Enc<'_, L>) -> IrResult<()> {
        Ok(())
    }
    #[inline]
    fn get<L: Layering>(_dec: &mut Dec<'a, L>) -> Self {
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
    const SHAPE: DiShape = DiShape::OptUInt;
    #[inline]
    fn out(&self) -> DiFieldOut<'a> {
        DiFieldOut::UInt(u64::from(self.unwrap_or(0)))
    }
    #[inline]
    fn absent() -> Self {
        None
    }
    #[inline]
    fn present(&self) -> bool {
        self.is_some()
    }
    #[inline]
    fn put<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<()> {
        enc.word(self.unwrap_or(0));
        Ok(())
    }
    #[inline]
    fn get<L: Layering>(dec: &mut Dec<'a, L>) -> Self {
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
    const SHAPE: DiShape = DiShape::Str;
    #[inline]
    fn out(&self) -> DiFieldOut<'a> {
        DiFieldOut::Str(self)
    }
    #[inline]
    fn absent() -> Self {
        &[]
    }
    #[inline]
    fn present(&self) -> bool {
        !self.is_empty()
    }
    fn put<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<()> {
        let id = enc.strings.get_or_insert_with(Box::default).intern(self)?;
        if id.raw() == *enc.string_count {
            // New: ids are handed out in order.
            *enc.string_count += 1;
            *enc.string_bytes += self.len() + size_of::<(u32, u32)>();
        }
        enc.word(id.raw());
        Ok(())
    }
    #[inline]
    fn get<L: Layering>(dec: &mut Dec<'a, L>) -> Self {
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
    const SHAPE: DiShape = DiShape::Node;
    #[inline]
    fn out(&self) -> DiFieldOut<'a> {
        DiFieldOut::Node(self.unwrap_or(DiId::NONE))
    }
    #[inline]
    fn absent() -> Self {
        None
    }
    #[inline]
    fn present(&self) -> bool {
        self.is_some()
    }
    #[inline]
    fn put<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<()> {
        enc.node(self.unwrap_or(DiId::NONE))
    }
    #[inline]
    fn get<L: Layering>(dec: &mut Dec<'a, L>) -> Self {
        Some(DiId::new(dec.word()))
    }
    fn assign(value: DiFieldValue<'a>) -> Option<Self> {
        match value {
            DiFieldValue::Node(id) => Some(id),
            _ => None,
        }
    }
    #[inline]
    fn refs(&self, visit: &mut impl FnMut(DiId)) {
        if let Some(id) = *self {
            visit(id);
        }
    }
    #[inline]
    fn ref_cells<L: Layering>(dec: &mut Dec<'a, L>, visit: &mut dyn FnMut(usize)) {
        visit(dec.cells.len());
        dec.word();
    }
}

impl<'a> Field<'a> for &'a [DiId] {
    const SHAPE: DiShape = DiShape::List;
    const CELLS: u8 = LIST_CELLS;
    #[inline]
    fn out(&self) -> DiFieldOut<'a> {
        DiFieldOut::List(self)
    }
    #[inline]
    fn absent() -> Self {
        &[]
    }
    #[inline]
    fn present(&self) -> bool {
        !self.is_empty()
    }
    fn put<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<()> {
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
    fn get<L: Layering>(dec: &mut Dec<'a, L>) -> Self {
        let len = dec.word() as usize;
        if let Some((list, rest)) = dec.cells.split_at_checked(len) {
            dec.cells = rest;
            list
        } else {
            dec.cells = &[];
            &[]
        }
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
    fn refs(&self, visit: &mut impl FnMut(DiId)) {
        self.iter().filter(|id| !id.is_none()).for_each(|&id| visit(id));
    }
    fn ref_cells<L: Layering>(dec: &mut Dec<'a, L>, visit: &mut dyn FnMut(usize)) {
        let list = Self::get(dec);
        let left = dec.cells.len();
        for (index, id) in list.iter().enumerate() {
            if !id.is_none() {
                visit(left + list.len() - index);
            }
        }
    }
}

impl<'a> Field<'a> for ExprId {
    const SHAPE: DiShape = DiShape::Expr;
    #[inline]
    fn out(&self) -> DiFieldOut<'a> {
        DiFieldOut::Expr(*self)
    }
    #[inline]
    fn absent() -> Self {
        ExprId::EMPTY
    }
    #[inline]
    fn present(&self) -> bool {
        *self != ExprId::EMPTY
    }
    fn put<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<()> {
        if self.raw() >= enc.exprs {
            return err(IrErrorKind::UndefinedDebugNode, format!("{self:?} does not exist"));
        }
        enc.word(self.raw());
        Ok(())
    }
    #[inline]
    fn get<L: Layering>(dec: &mut Dec<'a, L>) -> Self {
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
    const SHAPE: DiShape = DiShape::Const;
    #[inline]
    fn out(&self) -> DiFieldOut<'a> {
        DiFieldOut::Const(*self)
    }
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
    fn put<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<()> {
        enc.word(self.raw());
        Ok(())
    }
    #[inline]
    fn get<L: Layering>(dec: &mut Dec<'a, L>) -> Self {
        ConstId::new(dec.word())
    }
    fn assign(_value: DiFieldValue<'a>) -> Option<Self> {
        None
    }
}

impl<'a> Field<'a> for DiBound {
    const SHAPE: DiShape = DiShape::Bound;
    /// The bound's kind, then its value's two words.
    const CELLS: u8 = 3;
    #[inline]
    fn out(&self) -> DiFieldOut<'a> {
        DiFieldOut::Bound(*self)
    }
    #[inline]
    fn absent() -> Self {
        DiBound::None
    }
    #[inline]
    fn present(&self) -> bool {
        *self != DiBound::None
    }
    fn put<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<()> {
        match *self {
            DiBound::Node(id) if id.raw() >= enc.nodes => return dangling(id),
            DiBound::Expr(id) if id.raw() >= enc.exprs => {
                return err(IrErrorKind::UndefinedDebugNode, format!("{id:?} does not exist"));
            }
            DiBound::None | DiBound::Const(_) | DiBound::Node(_) | DiBound::Expr(_) => {}
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
    fn get<L: Layering>(dec: &mut Dec<'a, L>) -> Self {
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
    fn refs(&self, visit: &mut impl FnMut(DiId)) {
        if let DiBound::Node(id) = *self {
            visit(id);
        }
    }
    fn ref_cells<L: Layering>(dec: &mut Dec<'a, L>, visit: &mut dyn FnMut(usize)) {
        // The tag, then the value's low word (a node id) and high word.
        if dec.cells.first().is_some_and(|tag| tag.raw() == 2) {
            visit(dec.cells.len() - 1);
        }
        let _ = Self::get(dec);
    }
}

macro_rules! coded_field {
    ($($ty:ty: $shape:ident, $absent:expr, $code:expr, $make:expr;)*) => {$(
        impl<'a> Field<'a> for $ty {
            const SHAPE: DiShape = DiShape::$shape;
            #[inline]
            fn out(&self) -> DiFieldOut<'a> {
                DiFieldOut::$shape(*self)
            }
            #[inline]
            fn absent() -> Self {
                $absent
            }
            #[inline]
            fn present(&self) -> bool {
                *self != $absent
            }
            #[inline]
            fn put<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<()> {
                let code: fn(&$ty) -> u32 = $code;
                enc.word(code(self));
                Ok(())
            }
            #[inline]
            fn get<L: Layering>(dec: &mut Dec<'a, L>) -> Self {
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
    DiFlags: Flags, DiFlags(0), |flags| flags.0, |bits| u32::try_from(bits).ok().map(DiFlags);
    SpFlags: SpFlags, SpFlags(0), |flags| flags.0, |bits| u32::try_from(bits).ok().map(SpFlags);
    EmissionKind: EmissionKind, EmissionKind::NoDebug, |kind| kind.code(), EmissionKind::from_code;
    NameTableKind: NameTableKind, NameTableKind::Default, |kind| kind.code(), NameTableKind::from_code;
    ChecksumKind: ChecksumKind, ChecksumKind::None, |kind| kind.code(), ChecksumKind::from_code;
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
    pub(crate) const TEMPLATE_PARAM: u32 =
        of(DiKind::TemplateTypeParameter) | of(DiKind::TemplateValueParameter);
    /// LLVM's `DIType`.
    pub(crate) const TYPE: u32 = of(DiKind::BasicType) | DERIVED | COMPOSITE | SUBROUTINE | OTHER;
    /// LLVM's `DILocalScope`.
    pub(crate) const LOCAL_SCOPE: u32 =
        SUBPROGRAM | of(DiKind::LexicalBlock) | of(DiKind::LexicalBlockFile);
    /// LLVM's `DIScope`.
    pub(crate) const SCOPE: u32 = LOCAL_SCOPE | UNIT | FILE | of(DiKind::Namespace) | TYPE;
}

mod sealed {
    pub trait Sealed {}
}

/// A kind's record ([`DiSubprogram`], ...): what
/// [`crate::DebugInfo::view`] reads a node as. Implemented by the records
/// of this module only.
pub trait DiRecord: sealed::Sealed {
    /// The record's kind.
    const KIND: DiKind;
}

/// One stored node of the record `R`'s kind, its fields read one at a
/// time from the store (`spec/ir.md` §6.2): a method per field, named and
/// typed as the record's, that reads that field's cells and no other
/// field's. From [`crate::DebugInfo::view`]; what
/// [`crate::DebugInfo::node`] gives, without decoding every field.
pub struct DiView<'a, R, L: Layering = NoLayer> {
    pub(crate) tag: u16,
    /// The node's mask.
    pub(crate) mask: u32,
    /// The cells after the mask.
    pub(crate) cells: &'a [DiId],
    pub(crate) strings: Option<&'a StrPool<L>>,
    pub(crate) record: PhantomData<fn() -> R>,
}

impl<R, L: Layering> Clone for DiView<'_, R, L> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R, L: Layering> Copy for DiView<'_, R, L> {}

impl<R, L: Layering> std::fmt::Debug for DiView<'_, R, L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiView").field("tag", &self.tag).field("mask", &self.mask).finish()
    }
}

impl<'a, R, L: Layering> DiView<'a, R, L> {
    /// The field at mask bit `bit` of a record whose fields take `cells`
    /// ([`Field::CELLS`] each): its default when it is not stored, else
    /// its value, read after the stored fields before it are stepped
    /// over.
    #[inline]
    fn field<F: Field<'a>>(&self, bit: u32, cells: &[u8]) -> F {
        let at = 1u32 << bit;
        if self.mask & at == 0 {
            return F::absent();
        }
        let mut before = self.mask & (at - 1);
        let mut start = 0usize;
        while before != 0 {
            let field = before.trailing_zeros() as usize;
            before &= before - 1;
            start += match cells.get(field).copied().unwrap_or(0) {
                LIST_CELLS => 1 + self.cells.get(start).map_or(0, |len| len.raw() as usize),
                n => usize::from(n),
            };
        }
        let mut dec = Dec { cells: self.cells.get(start..).unwrap_or(&[]), strings: self.strings };
        F::get(&mut dec)
    }
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

            /// What kind of value the field LLVM calls `field` holds;
            /// `None` for a field the kind's record does not have.
            pub fn field_shape(self, field: &[u8]) -> Option<DiShape> {
                match self {
                    DiKind::Undefined => None,
                    $( DiKind::$variant => <$name>::shape(field), )*
                }
            }

            /// What kind of value the field at `index` of
            /// [`DiKind::fields`] holds; `None` past the end.
            #[inline]
            pub fn field_shape_at(self, index: usize) -> Option<DiShape> {
                match self {
                    DiKind::Undefined => None,
                    $( DiKind::$variant => <$name>::SHAPES.get(index).copied(), )*
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
                fn encode<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<(u16, u32)> {
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

                #[allow(unused_assignments, unused_variables, clippy::redundant_field_names, reason = "the last field's bit is not read; not every kind has a tag; the macro writes `tag: tag`")]
                fn decode<L: Layering>(tag: u16, mask: u32, dec: &mut Dec<'a, L>) -> Self {
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

                /// [`Self::set`] by the field's position in
                /// [`DiKind::fields`]: `None` past the end.
                #[allow(unused_assignments, unused_mut, reason = "the last field's position is not read; a kind may have no field")]
                #[inline]
                fn set_at(&mut self, index: usize, value: DiFieldValue<'a>) -> Option<bool> {
                    let mut at = 0usize;
                    $(
                        if nodes!(@second $tag, index == 0) {
                            let tag = <u16 as Field<'_>>::assign(value);
                            self.tag = tag.unwrap_or(self.tag);
                            return Some(tag.is_some());
                        }
                        at = 1;
                    )?
                    $(
                        if index == at {
                            let value = Field::assign(value);
                            if let Some(value) = value {
                                self.$field = value;
                            }
                            return Some(value.is_some());
                        }
                        at += 1;
                    )*
                    None
                }

                /// The shapes of [`DiKind::fields`], in their order.
                const SHAPES: &'static [DiShape] = &[
                    $(nodes!(@second $tag, DiShape::UInt),)?
                    $(<$fty as Field<'a>>::SHAPE,)*
                ];

                fn shape(field: &[u8]) -> Option<DiShape> {
                    $(
                        if nodes!(@second $tag, field == b"tag") {
                            return Some(DiShape::UInt);
                        }
                    )?
                    $(
                        if field == $fllvm.as_bytes() {
                            return Some(<$fty as Field<'_>>::SHAPE);
                        }
                    )*
                    None
                }

                fn each(&self, visit: &mut dyn FnMut(&'static str, DiFieldOut<'a>)) {
                    $( visit("tag", DiFieldOut::UInt(u64::from(nodes!(@second $tag, self.tag)))); )?
                    $(
                        if Field::present(&self.$field) {
                            visit($fllvm, Field::out(&self.$field));
                        }
                    )*
                }

                #[cfg(test)]
                fn refs(&self, visit: &mut dyn FnMut(&'static str, u32, DiId)) {
                    $(
                        Field::refs(&self.$field, &mut |id| {
                            visit($fllvm, nodes!(@class $($class)?), id);
                        });
                    )*
                }

                /// [`Self::refs`] of the node stored with `mask` (`dec`
                /// holds the cells after the mask), without decoding it.
                #[allow(unused_assignments, unused_variables, reason = "the last field's bit is not read; a kind may have no field")]
                #[inline]
                fn stored_refs<L: Layering>(mask: u32, dec: &mut Dec<'a, L>, visit: &mut impl FnMut(&'static str, u32, DiId)) {
                    let mut bit = 1u32;
                    $(
                        if mask & bit != 0 {
                            let value = <$fty as Field<'a>>::get(dec);
                            Field::refs(&value, &mut |id| {
                                visit($fllvm, nodes!(@class $($class)?), id);
                            });
                        }
                        bit <<= 1;
                    )*
                }

                #[allow(unused_assignments, unused_variables, reason = "the last field's bit is not read; a kind may have no field")]
                fn ref_cells<L: Layering>(mask: u32, dec: &mut Dec<'a, L>, visit: &mut dyn FnMut(usize)) {
                    let mut bit = 1u32;
                    $(
                        if mask & bit != 0 {
                            <$fty as Field<'a>>::ref_cells(dec, visit);
                        }
                        bit <<= 1;
                    )*
                }

                /// [`Field::CELLS`] of each field, in the mask's order.
                const CELLS: &'static [u8] = &[$(<$fty as Field<'a>>::CELLS,)*];

                /// Whether every field of `view` reads as this record's.
                #[cfg(test)]
                fn agrees<L: Layering>(&self, view: &DiView<'a, Self, L>) -> bool {
                    true $(&& nodes!(@second $tag, self.tag == view.tag()))?
                        $(&& self.$field == view.$field())*
                }
            }

            impl $(<$lt>)? sealed::Sealed for $name $(<$lt>)? {}

            impl $(<$lt>)? DiRecord for $name $(<$lt>)? {
                const KIND: DiKind = DiKind::$variant;
            }

            const _: () = {
                /// Each field's bit in the mask.
                #[allow(non_camel_case_types, dead_code, reason = "one variant per field, named as the field")]
                enum Bit { $($field,)* }

                impl<'a, L: Layering> DiView<'a, $name $(<$lt>)?, L> {
                    $(
                        #[doc = concat!("`tag:`, as [`", stringify!($name), "::tag`].")]
                        #[inline]
                        pub fn tag(&self) -> u16 {
                            nodes!(@second $tag, self.tag)
                        }
                    )?
                    $(
                        #[doc = concat!("`", $fllvm, ":`, as [`", stringify!($name), "::", stringify!($field), "`], read without the other fields.")]
                        #[inline]
                        pub fn $field(&self) -> $fty {
                            self.field::<$fty>(Bit::$field as u32, <$name>::CELLS)
                        }
                    )*
                }
            };
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

            pub(crate) fn encode<L: Layering>(&self, enc: &mut Enc<'_, L>) -> IrResult<(u16, u32)> {
                match self {
                    DiNode::Undefined => err(
                        IrErrorKind::InvalidDebugInfo,
                        "a node cannot be defined as undefined",
                    ),
                    $( DiNode::$variant(node) => node.encode(enc), )*
                }
            }

            pub(crate) fn decode<L: Layering>(kind: DiKind, tag: u16, mask: u32, dec: &mut Dec<'a, L>) -> DiNode<'a> {
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

            fn set_field_at(&mut self, index: usize, value: DiFieldValue<'a>) -> Option<bool> {
                match self {
                    DiNode::Undefined => None,
                    $( DiNode::$variant(node) => node.set_at(index, value), )*
                }
            }

            /// Calls `visit(field, value)` for every field that is not at
            /// its default, in the order of [`DiKind::fields`]; a kind
            /// with a tag always hands over its `tag` first.
            pub fn each_field(&self, visit: &mut dyn FnMut(&'static str, DiFieldOut<'a>)) {
                match self {
                    DiNode::Undefined => {}
                    $( DiNode::$variant(node) => node.each(visit), )*
                }
            }

            /// Whether node `id` of `debug`, read through
            /// [`crate::DebugInfo::view`], reads as this node, field by
            /// field.
            #[cfg(test)]
            pub(crate) fn agrees_with_view(&self, debug: &crate::DebugInfo, id: DiId) -> bool {
                match self {
                    DiNode::Undefined => true,
                    $( DiNode::$variant(node) => {
                        debug.view::<$name>(id).is_some_and(|view| node.agrees(&view))
                    } )*
                }
            }

            /// Calls `visit(field, class, id)` for every node this one
            /// names: LLVM's field name, the kinds the field may name (a
            /// `class` bit set) and the id. What [`DiNode::stored_refs`]
            /// reads without the node, which the tests hold it against.
            #[cfg(test)]
            pub(crate) fn refs(&self, visit: &mut dyn FnMut(&'static str, u32, DiId)) {
                match self {
                    DiNode::Undefined => {}
                    $( DiNode::$variant(node) => node.refs(visit), )*
                }
            }

            /// Walks the stored fields of a node of `kind` whose mask is
            /// `mask` (`dec` holds the cells after the mask), calling
            /// `visit` as [`Field::ref_cells`] does at every cell that
            /// holds a node id. `dec` is left after the node's last cell.
            /// [`DiNode::refs`] of the node of `kind` stored with `mask`
            /// (`dec` holds the cells after the mask; its strings are not
            /// read), without decoding the node.
            pub(crate) fn stored_refs<L: Layering>(kind: DiKind, mask: u32, dec: &mut Dec<'a, L>, visit: &mut impl FnMut(&'static str, u32, DiId)) {
                match kind {
                    DiKind::Undefined => {}
                    $( DiKind::$variant => <$name>::stored_refs(mask, dec, visit), )*
                }
            }

            pub(crate) fn ref_cells<L: Layering>(kind: DiKind, mask: u32, dec: &mut Dec<'a, L>, visit: &mut dyn FnMut(usize)) {
                match kind {
                    DiKind::Undefined => {}
                    $( DiKind::$variant => <$name>::ref_cells(mask, dec, visit), )*
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
        /// The `.dwo` name the frontend chose for split DWARF
        /// (`-gsplit-dwarf`, `spec/dwarf.md` §6.15). With `dwo_id` 0 it
        /// only names the file `-split-dwarf-file` writes; with a DWO id
        /// it names an external unit (a clang module).
        split_debug_filename "splitDebugFilename": &'a [u8],
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
        /// The DWO id of an external unit this unit stands for (a clang
        /// module's skeleton); 0 for an ordinary unit.
        dwo_id "dwoId": u64,
        /// Whether split DWARF repeats inlining in the skeleton unit
        /// (`-fsplit-dwarf-inlining`). LLVM's default is `true`
        /// ([`DiNode::llvm_default`]); clang and kiln write `false`.
        split_debug_inlining "splitDebugInlining": bool,
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
        /// Whether the function's locations carry key-instruction atoms
        /// (`atomGroup`, `atomRank`; LLVM 21 and later, WP-149): the
        /// line table marks statements by `spec/dwarf.md` §5.9 instead
        /// of §5.2.
        key_instructions "keyInstructions": bool,
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

impl DiKind {
    /// The position in [`DiKind::fields`] of the field LLVM calls
    /// `field`; `None` for a field the kind's record does not have.
    /// The search starts at `hint` and wraps around: a producer that
    /// reads fields in the record's order (LLVM writes them so) passes
    /// the position after the last one found and compares once.
    #[inline]
    pub fn field_index(self, field: &[u8], hint: usize) -> Option<usize> {
        let fields = self.fields();
        let start = if hint < fields.len() { hint } else { 0 };
        let same = |known: &&str| known.len() == field.len() && known.as_bytes() == field;
        if let Some(at) = fields[start..].iter().position(same) {
            return Some(start + at);
        }
        fields[..start].iter().position(same)
    }
}

impl<'a> DiNode<'a> {
    /// A node of `kind` as LLVM's parser starts one: every field at
    /// LLVM's default for an omitted field. That differs from
    /// [`DiNode::new`] in three places: a global variable is a definition,
    /// and so is a subprogram (LLVM's `isDefinition` defaults to `true`
    /// when neither it nor `spFlags` is written), and a compile unit's
    /// `splitDebugInlining` is `true`.
    pub fn llvm_default(kind: DiKind) -> DiNode<'a> {
        match kind {
            DiKind::GlobalVariable => DiNode::GlobalVariable(DiGlobalVariable {
                is_definition: true,
                ..Default::default()
            }),
            DiKind::Subprogram => DiNode::Subprogram(DiSubprogram {
                sp_flags: SpFlags::DEFINITION,
                ..Default::default()
            }),
            // WP-062: LLVM's `splitDebugInlining` defaults to `true`.
            DiKind::CompileUnit => DiNode::CompileUnit(DiCompileUnit {
                split_debug_inlining: true,
                ..Default::default()
            }),
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

    /// [`DiNode::set`] by the field's position in [`DiKind::fields`]
    /// ([`DiKind::field_index`]): the way a producer that has looked the
    /// name up once sets it without comparing it again. The older
    /// spellings of a subprogram's flags are no position: they go
    /// through [`DiNode::set`].
    ///
    /// # Errors
    /// As [`DiNode::set`]; the same messages for the same field.
    #[inline]
    pub fn set_at(&mut self, index: usize, value: DiFieldValue<'a>) -> IrResult<()> {
        match self.set_field_at(index, value) {
            Some(true) => Ok(()),
            done => self.set_at_failed(index, value, done.is_some()),
        }
    }

    #[cold]
    #[inline(never)]
    fn set_at_failed(&self, index: usize, value: DiFieldValue<'a>, shape: bool) -> IrResult<()> {
        let kind = self.kind();
        let what = if shape { "cannot be" } else { "is not a field hod_ir has; it was given" };
        let field = kind.fields().get(index).map_or_else(|| format!("#{index}"), |f| (*f).into());
        err(IrErrorKind::InvalidDebugInfo, format!("{}.{field} {what} {value:?}", kind.llvm_name()))
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

    /// WP-290: a field by position is the field by name: the same
    /// position from every hint, the same shape, the same node after a
    /// set and the same error after a refused one.
    #[test]
    fn fields_by_position_are_fields_by_name() {
        let values = [
            DiFieldValue::UInt(7),
            DiFieldValue::UInt(u64::MAX),
            DiFieldValue::Int(-3),
            DiFieldValue::Bool(true),
            DiFieldValue::Str(b"text"),
            DiFieldValue::Node(None),
            DiFieldValue::List(&[]),
        ];
        for &kind in DiKind::ALL {
            let fields = kind.fields();
            for (index, field) in fields.iter().enumerate() {
                for hint in 0..=fields.len() + 1 {
                    assert_eq!(kind.field_index(field.as_bytes(), hint), Some(index), "{field}");
                }
                assert_eq!(kind.field_shape_at(index), kind.field_shape(field.as_bytes()));
                for value in values {
                    let mut by_name = DiNode::llvm_default(kind);
                    let mut by_index = by_name;
                    let named = by_name.set(field.as_bytes(), value);
                    let indexed = by_index.set_at(index, value);
                    assert_eq!(named, indexed, "{kind:?}.{field} = {value:?}");
                    assert_eq!(by_name, by_index, "{kind:?}.{field} = {value:?}");
                }
            }
            assert_eq!(kind.field_index(b"colour", 0), None);
            assert_eq!(kind.field_index(b"", 3), None);
            assert_eq!(kind.field_shape_at(fields.len()), None);
            assert!(
                DiNode::llvm_default(kind).set_at(fields.len(), DiFieldValue::UInt(0)).is_err()
            );
        }
        // The older spellings of a subprogram's flags have no position.
        assert_eq!(DiKind::Subprogram.field_index(b"isDefinition", 0), None);
    }

    /// §6.2 (WP-311): a view reads every field as `node` decodes it, for
    /// any set of stored fields (lists of several lengths before the
    /// field read included); a view of another kind, or of an id that is
    /// not a node, is `None`.
    #[test]
    fn views_read_fields_as_nodes_do() {
        let mut d = crate::DebugInfo::default();
        let file = d.add(&DiFile { filename: b"t", ..Default::default() }.into()).unwrap();
        let expr = d.expr(&[crate::DiOp::Deref]).unwrap();
        let lists = [vec![file], vec![file, DiId::NONE, file], vec![DiId::NONE; 5]];
        let mut seed = 0x2545_f491_u32;
        let mut checked = 0;
        for &kind in DiKind::ALL {
            if kind == DiKind::Constant {
                continue;
            }
            let fields = kind.fields();
            for round in 0..96 {
                let mut node = DiNode::new(kind);
                for field in fields {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    // Round 0 sets every field it can; later rounds about half.
                    if round > 0 && seed >> 31 == 0 {
                        continue;
                    }
                    let list = &lists[(seed >> 8) as usize % lists.len()];
                    let values = [
                        DiFieldValue::Str(b"text"),
                        DiFieldValue::UInt(u64::from(seed >> 16) + 1),
                        DiFieldValue::UInt(0xfedc_ba98_7654_3210),
                        DiFieldValue::Int(-5),
                        DiFieldValue::Bool(true),
                        DiFieldValue::Node(Some(file)),
                        DiFieldValue::List(list),
                        DiFieldValue::Expr(expr),
                    ];
                    let start = (seed >> 4) as usize % values.len();
                    for value in values.iter().cycle().skip(start).take(values.len()) {
                        if node.set(field.as_bytes(), *value).is_ok() {
                            break;
                        }
                    }
                }
                let id = d.add_distinct(&node).unwrap();
                assert!(node.agrees_with_view(&d, id), "{kind:?}: {node:?}");
                checked += 1;
            }
            let id = d.add_distinct(&DiNode::new(kind)).unwrap();
            assert!(DiNode::new(kind).agrees_with_view(&d, id), "{kind:?}");
        }
        assert!(checked > 2000);
        assert!(d.view::<DiSubprogram>(file).is_none());
        assert!(d.view::<DiFile>(file).is_some());
        assert!(d.view::<DiFile>(DiId::new(u32::MAX - 1)).is_none());
    }

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
        assert_eq!(
            DiKind::File.fields(),
            ["filename", "directory", "checksumkind", "checksum", "source"]
        );
        assert_eq!(DiKind::BasicType.fields()[0], "tag");
        assert_eq!(DiKind::Subprogram.fields().len(), 17);
        assert_eq!(DiKind::Subprogram.fields()[16], "keyInstructions");
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
        assert_eq!(
            sp.sp_flags,
            SpFlags::DEFINITION | SpFlags::LOCAL_TO_UNIT | SpFlags::PURE_VIRTUAL
        );
        assert!(node.must_be_distinct());

        let kind = |result: IrResult<()>| result.unwrap_err().kind;
        assert_eq!(kind(node.set(b"line", DiFieldValue::Str(b"7"))), IrErrorKind::InvalidDebugInfo);
        assert_eq!(
            kind(node.set(b"line", DiFieldValue::UInt(1 << 32))),
            IrErrorKind::InvalidDebugInfo
        );
        assert_eq!(
            kind(node.set(b"annotations", DiFieldValue::Node(None))),
            IrErrorKind::InvalidDebugInfo
        );
        assert_eq!(
            kind(node.set(b"virtuality", DiFieldValue::UInt(3))),
            IrErrorKind::InvalidDebugInfo
        );

        let mut ty = DiNode::new(DiKind::DerivedType);
        ty.set(b"tag", DiFieldValue::UInt(0x0f)).unwrap();
        ty.set(b"size", DiFieldValue::UInt(1 << 40)).unwrap();
        ty.set(b"dwarfAddressSpace", DiFieldValue::UInt(0)).unwrap();
        assert_eq!(
            kind(ty.set(b"tag", DiFieldValue::UInt(1 << 16))),
            IrErrorKind::InvalidDebugInfo
        );
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
        assert_eq!(
            kind(unit.set(b"emissionKind", DiFieldValue::UInt(9))),
            IrErrorKind::InvalidDebugInfo
        );
        let mut value = DiNode::new(DiKind::Enumerator);
        value.set(b"value", DiFieldValue::UInt(u64::MAX)).unwrap();
        assert_eq!(value, DiNode::Enumerator(DiEnumerator { value: -1, ..Default::default() }));
        assert_eq!(
            kind(DiNode::Undefined.set(b"x", DiFieldValue::Bool(true))),
            IrErrorKind::InvalidDebugInfo
        );
    }
}
