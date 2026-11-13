//! Debug expressions (`spec/ir.md` §6.4): LLVM's `DIExpression`.
//!
//! An expression is stored as LLVM stores it, a list of 64-bit elements
//! (operation codes followed by their operands), and read as typed
//! operations. An element that is not a known operation is
//! [`DiOp::Other`]: kept, so that nothing is dropped, and named by
//! `classify`.

use std::fmt;

use crate::error::{IrErrorKind, IrResult, err};
use crate::intern::{Hasher64, InternTable, Layered, Layering, NoLayer, Seg, next_id, pool_start};

/// A debug expression of a module (§6.4). Equal expressions have equal
/// ids.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExprId(u32);

impl ExprId {
    /// The empty expression, `!DIExpression()`. Every module has it.
    pub const EMPTY: ExprId = ExprId(0);

    /// The id with raw value `raw`. For the code that owns the table.
    #[inline]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// The raw value.
    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// The id as a table index.
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl Default for ExprId {
    fn default() -> Self {
        ExprId::EMPTY
    }
}

impl fmt::Debug for ExprId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expr{}", self.0)
    }
}

macro_rules! ops {
    ($( $(#[$meta:meta])* $variant:ident $(($($operand:ident),+))? = $code:literal, $llvm:literal; )*) => {
        /// One operation of a debug expression (§6.4).
        ///
        /// Operands are the 64-bit elements LLVM stores; a signed operand
        /// is its two's-complement bits.
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum DiOp {
            $(
                $(#[$meta])*
                #[doc = concat!("\n\n`", $llvm, "`", $(" with the operands", $(" `", stringify!($operand), "`",)+)? ".")]
                $variant $(($(ops!(@u64 $operand)),+))?,
            )*
            /// An element that is none of the above: an operation hod
            /// has no meaning for, or an operand that followed one.
            /// `classify` names it `debug:DW_OP_<name or number>`.
            Other(u64),
        }

        /// The operations [`DiOp`] has a member for: the name LLVM
        /// writes, the code, and the number of operands.
        pub const DI_OPS: &[(&str, u64, u8)] = &[
            $( ($llvm, $code, 0 $($(+ ops!(@one $operand))+)?), )*
        ];

        impl DiOp {
            /// The operation's code: what LLVM stores as its first
            /// element.
            pub const fn code(self) -> u64 {
                match self {
                    $( ops!(@pattern $variant $(($($operand),+))?) => $code, )*
                    DiOp::Other(element) => element,
                }
            }

            /// The operands, in order.
            pub(crate) fn operands(self) -> ([u64; 2], usize) {
                match self {
                    $( DiOp::$variant $(($($operand),+))? => ops!(@list $($($operand),+)?), )*
                    DiOp::Other(_) => ([0, 0], 0),
                }
            }

            /// The operation that starts `elements`, and how many
            /// elements it takes. `None` when a known operation lacks
            /// operands.
            pub(crate) fn decode(elements: &[u64]) -> Option<(DiOp, usize)> {
                let (&code, rest) = elements.split_first()?;
                match code {
                    $( $code => ops!(@take rest, $variant $(($($operand),+))?), )*
                    other => Some((DiOp::Other(other), 1)),
                }
            }
        }
    };
    (@u64 $operand:ident) => { u64 };
    (@one $operand:ident) => { 1 };
    (@pattern $variant:ident) => { DiOp::$variant };
    (@pattern $variant:ident ($a:ident)) => { DiOp::$variant(_) };
    (@pattern $variant:ident ($a:ident, $b:ident)) => { DiOp::$variant(_, _) };
    (@list) => { ([0, 0], 0) };
    (@list $a:ident) => { ([$a, 0], 1) };
    (@list $a:ident, $b:ident) => { ([$a, $b], 2) };
    (@take $rest:ident, $variant:ident) => { Some((DiOp::$variant, 1)) };
    (@take $rest:ident, $variant:ident ($a:ident)) => {
        match $rest {
            [$a, ..] => Some((DiOp::$variant(*$a), 2)),
            _ => None,
        }
    };
    (@take $rest:ident, $variant:ident ($a:ident, $b:ident)) => {
        match $rest {
            [$a, $b, ..] => Some((DiOp::$variant(*$a, *$b), 3)),
            _ => None,
        }
    };
}

ops! {
    /// Replace the address on top of the stack by the pointer-sized
    /// value stored there.
    Deref = 0x06, "DW_OP_deref";
    /// Push an unsigned constant.
    Constu(value) = 0x10, "DW_OP_constu";
    /// Push a signed constant (the operand is its bits).
    Consts(bits) = 0x11, "DW_OP_consts";
    /// Duplicate the top of the stack.
    Dup = 0x12, "DW_OP_dup";
    /// Push a copy of the second entry.
    Over = 0x14, "DW_OP_over";
    /// Exchange the two top entries.
    Swap = 0x16, "DW_OP_swap";
    /// Rotate the three top entries.
    Rot = 0x17, "DW_OP_rot";
    /// Dereference in an address space given by the second entry.
    Xderef = 0x18, "DW_OP_xderef";
    /// Absolute value.
    Abs = 0x19, "DW_OP_abs";
    /// Bitwise and.
    And = 0x1a, "DW_OP_and";
    /// Signed division.
    Div = 0x1b, "DW_OP_div";
    /// Second entry minus top.
    Minus = 0x1c, "DW_OP_minus";
    /// Unsigned remainder.
    Mod = 0x1d, "DW_OP_mod";
    /// Product.
    Mul = 0x1e, "DW_OP_mul";
    /// Negation.
    Neg = 0x1f, "DW_OP_neg";
    /// Bitwise complement.
    Not = 0x20, "DW_OP_not";
    /// Bitwise or.
    Or = 0x21, "DW_OP_or";
    /// Sum.
    Plus = 0x22, "DW_OP_plus";
    /// Add an unsigned constant to the top of the stack.
    PlusUconst(addend) = 0x23, "DW_OP_plus_uconst";
    /// Shift left.
    Shl = 0x24, "DW_OP_shl";
    /// Logical shift right.
    Shr = 0x25, "DW_OP_shr";
    /// Arithmetic shift right.
    Shra = 0x26, "DW_OP_shra";
    /// Bitwise exclusive or.
    Xor = 0x27, "DW_OP_xor";
    /// Equal.
    Eq = 0x29, "DW_OP_eq";
    /// Greater or equal.
    Ge = 0x2a, "DW_OP_ge";
    /// Greater.
    Gt = 0x2b, "DW_OP_gt";
    /// Less or equal.
    Le = 0x2c, "DW_OP_le";
    /// Less.
    Lt = 0x2d, "DW_OP_lt";
    /// Not equal.
    Ne = 0x2e, "DW_OP_ne";
    /// Push zero.
    Lit0 = 0x30, "DW_OP_lit0";
    /// Dereference that many bytes.
    DerefSize(bytes) = 0x94, "DW_OP_deref_size";
    /// Push the address of the object being described.
    PushObjectAddress = 0x97, "DW_OP_push_object_address";
    /// What is on the stack is the variable's value, not its address.
    /// Last, or last before a fragment.
    StackValue = 0x9f, "DW_OP_stack_value";
    /// The expression describes `size` bits at bit `offset` of the
    /// variable. Always last.
    Fragment(offset, size) = 0x1000, "DW_OP_LLVM_fragment";
    /// Convert the top of the stack to an integer of `bits` bits with a
    /// `DW_ATE_*` encoding.
    Convert(bits, encoding) = 0x1001, "DW_OP_LLVM_convert";
    /// A memory tag offset (hardware-assisted address sanitizing).
    TagOffset(offset) = 0x1002, "DW_OP_LLVM_tag_offset";
    /// The value the next that-many operations describe is the one a
    /// register had on entry to the function. Always first.
    EntryValue(count) = 0x1003, "DW_OP_LLVM_entry_value";
    /// The variable is a pointer whose pointee the rest describes.
    ImplicitPointer = 0x1004, "DW_OP_LLVM_implicit_pointer";
    /// Push the record's value number that-many (§6.6): a record with an
    /// argument list names its values this way.
    Arg(index) = 0x1005, "DW_OP_LLVM_arg";
    /// Extract `size` bits at bit `offset`, sign-extended.
    ExtractBitsSext(offset, size) = 0x1006, "DW_OP_LLVM_extract_bits_sext";
    /// Extract `size` bits at bit `offset`, zero-extended.
    ExtractBitsZext(offset, size) = 0x1007, "DW_OP_LLVM_extract_bits_zext";
}

impl DiOp {
    /// The name LLVM writes for a known operation.
    pub fn name(self) -> Option<&'static str> {
        if matches!(self, DiOp::Other(_)) {
            return None;
        }
        DI_OPS.iter().find(|(_, code, _)| *code == self.code()).map(|(name, ..)| *name)
    }
}

/// The code of the operation LLVM writes as `name`, and its number of
/// operands, if [`DiOp`] has a member for it.
pub fn di_op_named(name: &[u8]) -> Option<(u64, u8)> {
    DI_OPS.iter().find(|(known, ..)| known.as_bytes() == name).map(|&(_, code, n)| (code, n))
}

/// The operations of one expression, in order.
#[derive(Clone, Debug)]
pub struct DiOps<'a> {
    elements: &'a [u64],
}

impl Iterator for DiOps<'_> {
    type Item = DiOp;

    fn next(&mut self) -> Option<DiOp> {
        // The table holds only expressions that decode: see `intern`.
        let (op, taken) = DiOp::decode(self.elements)?;
        self.elements = &self.elements[taken.min(self.elements.len())..];
        Some(op)
    }
}

/// The expressions of a module: one element pool, a span per expression.
#[derive(Clone, Debug, Default)]
pub(crate) struct ExprTable<L: Layering = NoLayer> {
    elements: Seg<u64, L>,
    /// `(start, len)` in `elements`; entry 0 is the empty expression and
    /// exists once any other does.
    spans: Seg<(u32, u32), L>,
    interner: Option<InternTable<L>>,
    /// The distinct `Other` elements met, for `classify`.
    others: Vec<u64>,
}

impl ExprTable {
    pub(crate) fn seed(&self) -> ExprTable<Layered> {
        ExprTable {
            elements: self.elements.seed(),
            spans: self.spans.seed(),
            interner: self.interner.as_ref().map(InternTable::seed),
            others: self.others.clone(),
        }
    }
}

impl ExprTable<Layered> {
    pub(crate) fn fork(&self) -> ExprTable<Layered> {
        ExprTable {
            elements: self.elements.fork(),
            spans: self.spans.fork(),
            interner: self.interner.as_ref().map(InternTable::fork),
            others: self.others.clone(),
        }
    }

    /// The elements of the expressions from span `from` on, each owned.
    pub(crate) fn owned_since(&self, from: usize) -> Vec<Box<[u64]>> {
        (from..self.spans.len()).map(|id| self.elements(ExprId(id as u32)).into()).collect()
    }
}

impl<L: Layering> ExprTable<L> {
    /// The number of expressions, the empty one included.
    pub(crate) fn len(&self) -> usize {
        self.spans.len().max(1)
    }

    /// The elements of `id`; empty for an id that names nothing.
    #[inline]
    pub(crate) fn elements(&self, id: ExprId) -> &[u64] {
        match self.spans.get(id.index()) {
            Some(&(start, len)) => self.elements.slice(start as usize, len as usize),
            None => &[],
        }
    }

    pub(crate) fn ops(&self, id: ExprId) -> DiOps<'_> {
        DiOps { elements: self.elements(id) }
    }

    pub(crate) fn others(&self) -> &[u64] {
        &self.others
    }

    pub(crate) fn bytes(&self) -> usize {
        self.elements.bytes() + self.spans.bytes()
    }

    /// The id of the expression whose elements were just pushed at
    /// `start..`, which are removed again if an equal one exists.
    fn intern(&mut self, start: usize) -> IrResult<ExprId> {
        let result = self.intern_tail(start);
        if !matches!(result, Ok((_, true))) {
            self.elements.truncate(start);
        }
        result.map(|(id, _)| id)
    }

    /// The id for the tail, and whether the tail was kept.
    fn intern_tail(&mut self, start: usize) -> IrResult<(ExprId, bool)> {
        let tail = self.elements.from(start);
        if tail.is_empty() {
            return Ok((ExprId::EMPTY, false));
        }
        // Operand counts: a known operation has all its operands.
        let mut rest = tail;
        while !rest.is_empty() {
            let Some((op, taken)) = DiOp::decode(rest) else {
                let name = DiOp::decode(&[rest[0], 0, 0]).and_then(|(op, _)| op.name());
                return err(
                    IrErrorKind::InvalidDebugInfo,
                    format!("{} lacks an operand", name.unwrap_or("an operation")),
                );
            };
            if let DiOp::Other(element) = op
                && !self.others.contains(&element)
            {
                self.others.push(element);
            }
            rest = &rest[taken..];
        }
        let tail = self.elements.from(start);
        let mut hasher = Hasher64::new(0xd1e);
        for &element in tail {
            hasher.word(element);
        }
        let hash = hasher.finish();
        let interner = self.interner.get_or_insert_with(InternTable::new);
        let found = interner.find(hash, |id| {
            let (at, len) = self.spans[id as usize];
            self.elements.slice(at as usize, len as usize) == tail
        });
        if let Some(id) = found {
            return Ok((ExprId(id), false));
        }
        if self.spans.is_empty() {
            self.spans.push((0, 0));
        }
        let id = next_id(self.spans.len(), "debug expression")?;
        let at = pool_start(start, tail.len(), "debug expression element")?;
        self.spans.push((at, tail.len() as u32));
        interner.insert(hash, id);
        Ok((ExprId(id), true))
    }

    /// The expression with these operations.
    pub(crate) fn add_ops(&mut self, ops: &[DiOp]) -> IrResult<ExprId> {
        let start = self.elements.len();
        for op in ops {
            self.elements.push(op.code());
            let (operands, count) = op.operands();
            self.elements.extend_from_slice(&operands[..count]);
        }
        self.intern(start)
    }

    /// The expression with these elements.
    pub(crate) fn add_elements(&mut self, elements: &[u64]) -> IrResult<ExprId> {
        let start = self.elements.len();
        self.elements.extend_from_slice(elements);
        self.intern(start)
    }

    // ---- `spec/ir.md` §5.1: reading bodies in parallel ----

    /// The number of spans, the empty expression's included once made:
    /// where a fork's next expression goes.
    pub(crate) fn mark(&self) -> usize {
        self.spans.len()
    }
}

/// What is wrong with the placement of `ops`, by the rules of LLVM's
/// `DIExpression::isValid`: an entry value first (or right after a
/// leading `DW_OP_LLVM_arg 0`) and covering one operation, a stack value
/// last or right before a fragment, a fragment last, a swap not alone.
pub(crate) fn order_error(ops: DiOps<'_>) -> Option<&'static str> {
    let count = ops.clone().count();
    let leading_arg = matches!(ops.clone().next(), Some(DiOp::Arg(0)));
    let mut rest = ops.clone();
    for (index, op) in ops.enumerate() {
        rest.next();
        let last = index + 1 == count;
        match op {
            DiOp::EntryValue(_) if index != usize::from(leading_arg) => {
                return Some("DW_OP_LLVM_entry_value is not first");
            }
            DiOp::EntryValue(covered) if covered != 1 => {
                return Some("DW_OP_LLVM_entry_value covers other than one operation");
            }
            DiOp::Fragment(..) if !last => return Some("DW_OP_LLVM_fragment is not last"),
            DiOp::StackValue
                if !last && !matches!(rest.clone().next(), Some(DiOp::Fragment(..))) =>
            {
                return Some("DW_OP_stack_value is not last or before the fragment");
            }
            DiOp::Swap if count == 1 => return Some("DW_OP_swap has nothing to exchange"),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §6.4: expressions are interned; the empty one is id 0 and costs
    /// nothing.
    #[test]
    fn expressions_are_interned() {
        let mut table = <ExprTable>::default();
        assert_eq!(table.add_ops(&[]).unwrap(), ExprId::EMPTY);
        assert_eq!(table.bytes(), 0);
        assert_eq!(table.len(), 1);
        let deref = table.add_ops(&[DiOp::Deref, DiOp::PlusUconst(4)]).unwrap();
        assert_eq!(table.add_elements(&[0x06, 0x23, 4]).unwrap(), deref);
        assert_ne!(deref, ExprId::EMPTY);
        assert_eq!(table.elements(deref), &[0x06, 0x23, 4]);
        assert_eq!(table.ops(deref).collect::<Vec<_>>(), [DiOp::Deref, DiOp::PlusUconst(4)]);
        assert_eq!(table.ops(ExprId::EMPTY).count(), 0);
        let fragment = table.add_ops(&[DiOp::Fragment(32, 16)]).unwrap();
        assert_eq!(table.elements(fragment), &[0x1000, 32, 16]);
        assert_eq!(table.len(), 3);
        assert_eq!(table.elements.len(), 6, "a duplicate leaves nothing behind");
        assert!(table.elements(ExprId::new(3)).is_empty(), "an id that names nothing");
        assert_eq!(format!("{fragment:?}"), "expr2");
    }

    /// §6.4: an unknown element is kept as `Other`; a known operation
    /// without its operands is refused.
    #[test]
    fn unknown_elements_are_kept_and_counts_checked() {
        let mut table = <ExprTable>::default();
        let id = table.add_elements(&[0x90, 7, 0x06]).unwrap();
        assert_eq!(
            table.ops(id).collect::<Vec<_>>(),
            [DiOp::Other(0x90), DiOp::Other(7), DiOp::Deref]
        );
        assert_eq!(table.others(), &[0x90, 7]);
        let error = table.add_elements(&[0x06, 0x1000, 8]).unwrap_err();
        assert_eq!(error.kind, IrErrorKind::InvalidDebugInfo);
        assert!(error.message.contains("DW_OP_LLVM_fragment"), "{error}");
        assert_eq!(table.elements.len(), 3, "a refused expression leaves nothing behind");
        assert_eq!(table.add_elements(&[0x23]).unwrap_err().kind, IrErrorKind::InvalidDebugInfo);
    }

    /// §6.4: every member has a distinct code and converts both ways.
    #[test]
    fn operations_round_trip_through_their_codes() {
        for (index, &(name, code, operands)) in DI_OPS.iter().enumerate() {
            assert!(DI_OPS[..index].iter().all(|other| other.1 != code && other.0 != name));
            let (op, taken) = DiOp::decode(&[code, 11, 22]).unwrap();
            assert_eq!(taken, 1 + usize::from(operands), "{name}");
            assert_eq!(op.code(), code);
            assert_eq!(op.name(), Some(name));
            assert_eq!(op.operands().1, usize::from(operands));
            assert_eq!(di_op_named(name.as_bytes()), Some((code, operands)));
        }
        assert_eq!(DiOp::Other(0x90).name(), None);
        assert_eq!(di_op_named(b"DW_OP_regx"), None);
    }

    /// §6.4: LLVM's placement rules.
    #[test]
    fn placement_rules() {
        let mut table = <ExprTable>::default();
        let mut check = |ops: &[DiOp]| {
            let id = table.add_ops(ops).unwrap();
            order_error(table.ops(id)).is_none()
        };
        assert!(check(&[DiOp::Constu(1), DiOp::StackValue, DiOp::Fragment(0, 8)]));
        assert!(check(&[DiOp::EntryValue(1), DiOp::Deref]));
        assert!(check(&[DiOp::Arg(0), DiOp::EntryValue(1), DiOp::Deref]));
        assert!(!check(&[DiOp::Arg(1), DiOp::EntryValue(1)]));
        assert!(!check(&[DiOp::EntryValue(2), DiOp::Deref]));
        assert!(!check(&[DiOp::Deref, DiOp::EntryValue(1)]));
        assert!(check(&[DiOp::Constu(1), DiOp::StackValue]));
        assert!(!check(&[DiOp::Constu(1), DiOp::StackValue, DiOp::Deref]));
        assert!(!check(&[DiOp::StackValue, DiOp::Deref, DiOp::Fragment(0, 8)]));
        assert!(!check(&[DiOp::Fragment(0, 8), DiOp::Deref]));
        assert!(!check(&[DiOp::Swap]));
        assert!(check(&[DiOp::Dup, DiOp::Swap]));
    }
}
