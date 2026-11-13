//! The constant pool (`spec/ir.md` §4): interned constants in normal form.
//!
//! Equal constants have equal [`ConstId`]s, so each kind has one canonical
//! representation:
//!
//! - **Integers.** A value is its low `N` bits; [`ConstPool::int`] reduces
//!   its argument modulo `2^N`, so `i8 -1` and `i8 255` are one constant.
//!   Widths up to 64 are [`Const::Int`] with the value zero-extended;
//!   wider ones are [`Const::WideInt`] with exactly `ceil(N / 64)`
//!   little-endian words and the bits above `N` clear. The view tells the
//!   two apart by width, never by value: `i128 5` is a `WideInt`.
//! - **Floats** are their bit pattern: `-0.0` and `+0.0` differ, and so do
//!   two NaNs with different payloads.
//! - **Zero.** Integer zero, float `+0.0` and `null` are the zero of their
//!   types. Every other zero (aggregates, vectors, target types) is
//!   [`Const::Zero`], and an aggregate, splat or byte string whose elements
//!   are all zero is canonicalized to it. Likewise an aggregate of only
//!   `poison`, or only `undef`, is that constant of the aggregate type, as
//!   in LLVM.
//! - **Arrays of `i8`** whose elements are all integers are
//!   [`Const::Bytes`], whichever way they were built.
//! - **Fixed vectors** whose elements are all the same constant are
//!   [`Const::Splat`], the only form a scalable vector has.
//! - **Addresses** are `symbol + addend` (§4.2); `fold.rs` reduces LLVM's
//!   constant expressions to them.
//!
//! Constructors validate their operands against the type table and return
//! `IrError`s; they never panic on bad input.

use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::{BlockId, ConstId, SymId, TypeId};
use crate::intern::{Hasher64, InternTable, Layered, Layering, NoLayer, Seg, next_id, pool_start};
use crate::types::{TypeKind, TypeTable};
use crate::wide;

/// The operator of a constant expression LLVM 19 to 22 can express.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum ConstOp {
    /// `trunc`.
    Trunc,
    /// `zext` (bitcode from older producers).
    ZExt,
    /// `sext` (bitcode from older producers).
    SExt,
    /// `ptrtoint`.
    PtrToInt,
    /// `ptrtoaddr` (LLVM 22).
    PtrToAddr,
    /// `inttoptr`.
    IntToPtr,
    /// `bitcast`.
    BitCast,
    /// `addrspacecast`.
    AddrSpaceCast,
    /// `getelementptr`.
    GetElementPtr,
    /// `add`.
    Add,
    /// `sub`.
    Sub,
    /// `mul`.
    Mul,
    /// `xor`.
    Xor,
    /// `and` (bitcode from older producers).
    And,
    /// `or` (bitcode from older producers).
    Or,
    /// `shl` (bitcode from older producers).
    Shl,
    /// `extractelement`.
    ExtractElement,
    /// `insertelement`.
    InsertElement,
    /// `shufflevector`.
    ShuffleVector,
}

impl ConstOp {
    /// The LLVM keyword, which is also the name `classify` reports.
    pub fn name(self) -> &'static str {
        match self {
            ConstOp::Trunc => "trunc",
            ConstOp::ZExt => "zext",
            ConstOp::SExt => "sext",
            ConstOp::PtrToInt => "ptrtoint",
            ConstOp::PtrToAddr => "ptrtoaddr",
            ConstOp::IntToPtr => "inttoptr",
            ConstOp::BitCast => "bitcast",
            ConstOp::AddrSpaceCast => "addrspacecast",
            ConstOp::GetElementPtr => "getelementptr",
            ConstOp::Add => "add",
            ConstOp::Sub => "sub",
            ConstOp::Mul => "mul",
            ConstOp::Xor => "xor",
            ConstOp::And => "and",
            ConstOp::Or => "or",
            ConstOp::Shl => "shl",
            ConstOp::ExtractElement => "extractelement",
            ConstOp::InsertElement => "insertelement",
            ConstOp::ShuffleVector => "shufflevector",
        }
    }

    /// Whether the operator converts one operand to a type.
    pub fn is_cast(self) -> bool {
        (ConstOp::Trunc as u8..=ConstOp::AddrSpaceCast as u8).contains(&(self as u8))
    }

    /// Whether the operator takes two operands of one integer type.
    pub fn is_binary(self) -> bool {
        (ConstOp::Add as u8..=ConstOp::Shl as u8).contains(&(self as u8))
    }
}

/// A constant, as read from the pool with [`ConstPool::get`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Const<'a> {
    /// An integer at most 64 bits wide. `value` is zero-extended.
    Int {
        /// The integer type.
        ty: TypeId,
        /// The value, with the bits above the type's width clear.
        value: u64,
    },
    /// An integer more than 64 bits wide.
    WideInt {
        /// The integer type.
        ty: TypeId,
        /// `ceil(width / 64)` little-endian words, top bits clear.
        words: &'a [u64],
    },
    /// A `half`, `bfloat`, `float` or `double`, as its bit pattern.
    Float {
        /// The floating-point type.
        ty: TypeId,
        /// The encoding, in the low bits.
        bits: u64,
    },
    /// An `fp128` or `x86_fp80`, as its bit pattern.
    WideFloat {
        /// The floating-point type.
        ty: TypeId,
        /// The encoding, in the low 128 or 80 bits.
        bits: u128,
    },
    /// `null` of a pointer type.
    Null(TypeId),
    /// The zero of an aggregate, vector or target extension type.
    Zero(TypeId),
    /// `undef`. For code generation, any value: hod uses zero.
    Undef(TypeId),
    /// `poison`. For code generation the same as `undef`.
    Poison(TypeId),
    /// `none`, the `token` constant.
    NoneToken,
    /// An array, struct or fixed vector by its elements; which one is the
    /// type's kind.
    Aggregate {
        /// The array, struct or vector type.
        ty: TypeId,
        /// One constant per element or field.
        elems: &'a [ConstId],
    },
    /// A vector, fixed or scalable, whose every element is `elem`.
    Splat {
        /// The vector type.
        ty: TypeId,
        /// The element.
        elem: ConstId,
    },
    /// An `[N x i8]` by its bytes.
    Bytes {
        /// The array type.
        ty: TypeId,
        /// The contents, `N` bytes.
        bytes: &'a [u8],
    },
    /// The address of a symbol plus a byte offset. Type `ptr`.
    Addr {
        /// The symbol.
        sym: SymId,
        /// Signed byte offset from the symbol.
        addend: i64,
    },
    /// `blockaddress(func, block)`. Type `ptr`.
    BlockAddr {
        /// The function.
        func: SymId,
        /// A block of that function's body.
        block: BlockId,
    },
    /// `a - b + addend`, truncated to the width of `ty`.
    SymDiff {
        /// The integer type of the difference; its width is the spec's
        /// `width`. At most 64 bits.
        ty: TypeId,
        /// The symbol added.
        a: SymId,
        /// The symbol subtracted.
        b: SymId,
        /// The constant added, sign-extended from the width of `ty`.
        addend: i64,
        /// Whether `a` is `dso_local_equivalent @a`: the difference may be
        /// taken to a PLT entry of `a` (relative vtables).
        plt: bool,
    },
    /// `ptrtoint` of an address constant: the address, zero-extended or
    /// truncated to `ty`.
    IntOfAddr {
        /// The integer type.
        ty: TypeId,
        /// An `Addr`, `BlockAddr`, `DsoLocalEquivalent` or `NoCfi`.
        addr: ConstId,
    },
    /// `inttoptr` of a non-zero integer: an absolute address. Type `ptr`.
    AddrOfInt {
        /// The address.
        value: u64,
    },
    /// `dso_local_equivalent @f`. Type `ptr`.
    DsoLocalEquivalent(SymId),
    /// `no_cfi @f`. Type `ptr`.
    NoCfi(SymId),
    /// A constant expression with no normal form. `classify` rejects it by
    /// the operator's name.
    Expr {
        /// The operator.
        op: ConstOp,
        /// The result type.
        ty: TypeId,
        /// The source element type of a `getelementptr`.
        source_ty: Option<TypeId>,
        /// The operands.
        args: &'a [ConstId],
    },
}

/// A stored constant: 16 bytes. Variable-length parts are ranges of the
/// pool's shared vectors.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Repr {
    Int {
        ty: TypeId,
        value: u64,
    },
    /// `len` words at `words` in the word pool.
    WideInt {
        ty: TypeId,
        words: u32,
        len: u32,
    },
    Float {
        ty: TypeId,
        bits: u64,
    },
    /// Two words at `words` in the word pool.
    WideFloat {
        ty: TypeId,
        words: u32,
    },
    Null(TypeId),
    Zero(TypeId),
    Undef(TypeId),
    Poison(TypeId),
    NoneToken,
    /// `len` ids at `elems` in the element pool.
    Aggregate {
        ty: TypeId,
        elems: u32,
        len: u32,
    },
    Splat {
        ty: TypeId,
        elem: ConstId,
    },
    /// `len` bytes at `start` in the byte pool.
    Bytes {
        ty: TypeId,
        start: u32,
        len: u32,
    },
    Addr {
        sym: SymId,
        addend: i64,
    },
    BlockAddr {
        func: SymId,
        block: BlockId,
    },
    /// Two words at `words`: `a | b << 32`, then the addend.
    SymDiff {
        ty: TypeId,
        words: u32,
        plt: bool,
    },
    IntOfAddr {
        ty: TypeId,
        addr: ConstId,
    },
    AddrOfInt {
        value: u64,
    },
    DsoLocalEquivalent(SymId),
    NoCfi(SymId),
    /// At `args` in the element pool: the source type's raw id (or
    /// `u32::MAX`), then `len` operand ids.
    Expr {
        ty: TypeId,
        op: ConstOp,
        args: u32,
        len: u32,
    },
}

/// The kind tags that seed the hash of entries with pooled parts.
mod seed {
    pub(super) const WIDE_INT: u64 = 101;
    pub(super) const WIDE_FLOAT: u64 = 102;
    pub(super) const AGGREGATE: u64 = 103;
    pub(super) const BYTES: u64 = 104;
    pub(super) const SYM_DIFF: u64 = 105;
    pub(super) const EXPR: u64 = 106;
}

const NO_SOURCE_TYPE: u32 = u32::MAX;

/// The interned constants of a module.
#[derive(Clone, Debug, Default)]
pub struct ConstPool<L: Layering = NoLayer> {
    data: Seg<Repr, L>,
    words: Seg<u64, L>,
    elems: Seg<ConstId, L>,
    bytes: Seg<u8, L>,
    interner: InternTable<L>,
    /// The `BlockAddr` constants, in id order: the address-taken mark
    /// (§4.2, WP-064).
    block_addrs: Seg<ConstId, L>,
}

impl ConstPool {
    /// An empty pool.
    pub fn new() -> ConstPool {
        ConstPool::default()
    }

    /// The base of the forks of parallel reading: a copy.
    pub(crate) fn seed(&self) -> ConstPool<Layered> {
        ConstPool {
            data: self.data.seed(),
            words: self.words.seed(),
            elems: self.elems.seed(),
            bytes: self.bytes.seed(),
            interner: self.interner.seed(),
            block_addrs: self.block_addrs.seed(),
        }
    }

    /// Interns, in order, the constants of `tail` (a fork's constants
    /// from `first`), with the type and constant ids they name
    /// translated, and appends the id each became to `map`: the same
    /// entries, made in the same order, as in the fork. Ids below
    /// `base` (`types_base` for types) are the base's, the same in both.
    pub(crate) fn adopt(
        &mut self,
        tail: &ConstPool<Layered>,
        first: usize,
        map: &mut Vec<ConstId>,
        base: usize,
        types: &[TypeId],
        types_base: usize,
    ) -> IrResult<()> {
        let ty_of = |ty: TypeId| -> TypeId {
            if ty.index() < types_base { ty } else { types[ty.index() - types_base] }
        };
        let id_of = |map: &[ConstId], id: ConstId| -> ConstId {
            if id.index() < base { id } else { map[id.index() - base] }
        };
        let mut scratch = Vec::new();
        for id in first..tail.data.len() {
            let repr = tail.data[id];
            let made = match tail.get(ConstId::new(id as u32)) {
                Const::Int { ty, value } => self.intern(Repr::Int { ty: ty_of(ty), value })?,
                Const::WideInt { ty, words } => {
                    let (ty, len) = (ty_of(ty), words.len() as u32);
                    self.intern_words(
                        seed::WIDE_INT,
                        ty,
                        0,
                        words,
                        |repr| match *repr {
                            Repr::WideInt { ty: t, words, len } if t == ty => {
                                Some((words, len as usize))
                            }
                            _ => None,
                        },
                        |words| Repr::WideInt { ty, words, len },
                    )?
                }
                Const::Float { ty, bits } => self.intern(Repr::Float { ty: ty_of(ty), bits })?,
                Const::WideFloat { ty, bits } => {
                    let ty = ty_of(ty);
                    self.intern_words(
                        seed::WIDE_FLOAT,
                        ty,
                        0,
                        &[bits as u64, (bits >> 64) as u64],
                        |repr| match *repr {
                            Repr::WideFloat { ty: t, words } if t == ty => Some((words, 2)),
                            _ => None,
                        },
                        |words| Repr::WideFloat { ty, words },
                    )?
                }
                Const::Aggregate { ty, elems } => {
                    scratch.clear();
                    scratch.extend(elems.iter().map(|&elem| id_of(map, elem)));
                    self.intern_aggregate(ty_of(ty), &scratch)?
                }
                Const::Bytes { ty, bytes } => self.bytes_of_type(ty_of(ty), bytes)?,
                Const::SymDiff { ty, a, b, addend, plt } => {
                    let ty = ty_of(ty);
                    let words = [u64::from(a.raw()) | (u64::from(b.raw()) << 32), addend as u64];
                    self.intern_words(
                        seed::SYM_DIFF,
                        ty,
                        u32::from(plt),
                        &words,
                        |repr| match *repr {
                            Repr::SymDiff { ty: t, words, plt: p } if t == ty && p == plt => {
                                Some((words, 2))
                            }
                            _ => None,
                        },
                        |words| Repr::SymDiff { ty, words, plt },
                    )?
                }
                Const::Expr { op, ty, source_ty, args } => {
                    scratch.clear();
                    scratch.extend(args.iter().map(|&arg| id_of(map, arg)));
                    self.expr(op, ty_of(ty), source_ty.map(ty_of), &scratch)?
                }
                // Through `block_addr`, which keeps the address-taken mark.
                Const::BlockAddr { func, block } => self.block_addr(func, block)?,
                // The entries without a pooled part: the stored form with
                // its ids translated.
                _ => self.intern(match repr {
                    Repr::Null(ty) => Repr::Null(ty_of(ty)),
                    Repr::Zero(ty) => Repr::Zero(ty_of(ty)),
                    Repr::Undef(ty) => Repr::Undef(ty_of(ty)),
                    Repr::Poison(ty) => Repr::Poison(ty_of(ty)),
                    Repr::Splat { ty, elem } => {
                        Repr::Splat { ty: ty_of(ty), elem: id_of(map, elem) }
                    }
                    Repr::IntOfAddr { ty, addr } => {
                        Repr::IntOfAddr { ty: ty_of(ty), addr: id_of(map, addr) }
                    }
                    Repr::NoneToken
                    | Repr::Addr { .. }
                    | Repr::AddrOfInt { .. }
                    | Repr::DsoLocalEquivalent(_)
                    | Repr::NoCfi(_) => repr,
                    _ => unreachable!("an entry with a pooled part is matched above"),
                })?,
            };
            map.push(made);
        }
        Ok(())
    }
}

impl ConstPool<Layered> {
    /// A pool with this one's constants and none of its own.
    pub(crate) fn fork(&self) -> ConstPool<Layered> {
        ConstPool {
            data: self.data.fork(),
            words: self.words.fork(),
            elems: self.elems.fork(),
            bytes: self.bytes.fork(),
            interner: self.interner.fork(),
            block_addrs: self.block_addrs.fork(),
        }
    }

    /// Whether no constant was added since the pool was forked.
    pub(crate) fn local_is_empty(&self) -> bool {
        self.data.local_is_empty()
    }

    /// The constants from `from` on, copied, for [`ConstPool::adopt`].
    pub(crate) fn tail(&self, from: usize) -> ConstPool<Layered> {
        // Each entry appends its pooled part when it is made, so the
        // tail's parts are the pools' ends from the first one it has.
        let (mut words, mut elems, mut bytes) =
            (self.words.len(), self.elems.len(), self.bytes.len());
        for id in from..self.data.len() {
            match self.data[id] {
                Repr::WideInt { words: at, .. }
                | Repr::WideFloat { words: at, .. }
                | Repr::SymDiff { words: at, .. } => words = words.min(at as usize),
                Repr::Aggregate { elems: at, .. } | Repr::Expr { args: at, .. } => {
                    elems = elems.min(at as usize);
                }
                Repr::Bytes { start, .. } => bytes = bytes.min(start as usize),
                _ => {}
            }
        }
        ConstPool {
            data: self.data.tail(from),
            words: self.words.tail(words),
            elems: self.elems.tail(elems),
            bytes: self.bytes.tail(bytes),
            interner: InternTable::new(),
            block_addrs: Seg::default(),
        }
    }
}

impl<L: Layering> ConstPool<L> {
    /// Number of constants in the pool.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the pool has no constants.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    // ---- reading ----

    /// The constant `id`.
    pub fn get(&self, id: ConstId) -> Const<'_> {
        match self.data[id.index()] {
            Repr::Int { ty, value } => Const::Int { ty, value },
            Repr::WideInt { ty, words, len } => {
                Const::WideInt { ty, words: self.words.slice(words as usize, len as usize) }
            }
            Repr::Float { ty, bits } => Const::Float { ty, bits },
            Repr::WideFloat { ty, words } => {
                let at = words as usize;
                let bits = u128::from(self.words[at]) | (u128::from(self.words[at + 1]) << 64);
                Const::WideFloat { ty, bits }
            }
            Repr::Null(ty) => Const::Null(ty),
            Repr::Zero(ty) => Const::Zero(ty),
            Repr::Undef(ty) => Const::Undef(ty),
            Repr::Poison(ty) => Const::Poison(ty),
            Repr::NoneToken => Const::NoneToken,
            Repr::Aggregate { ty, elems, len } => {
                Const::Aggregate { ty, elems: self.elems.slice(elems as usize, len as usize) }
            }
            Repr::Splat { ty, elem } => Const::Splat { ty, elem },
            Repr::Bytes { ty, start, len } => {
                Const::Bytes { ty, bytes: self.bytes.slice(start as usize, len as usize) }
            }
            Repr::Addr { sym, addend } => Const::Addr { sym, addend },
            Repr::BlockAddr { func, block } => Const::BlockAddr { func, block },
            Repr::SymDiff { ty, words, plt } => {
                let at = words as usize;
                let (pair, addend) = (self.words[at], self.words[at + 1]);
                Const::SymDiff {
                    ty,
                    a: SymId::new(pair as u32),
                    b: SymId::new((pair >> 32) as u32),
                    addend: addend as i64,
                    plt,
                }
            }
            Repr::IntOfAddr { ty, addr } => Const::IntOfAddr { ty, addr },
            Repr::AddrOfInt { value } => Const::AddrOfInt { value },
            Repr::DsoLocalEquivalent(sym) => Const::DsoLocalEquivalent(sym),
            Repr::NoCfi(sym) => Const::NoCfi(sym),
            Repr::Expr { ty, op, args, len } => {
                let at = args as usize;
                let source = self.elems[at].raw();
                Const::Expr {
                    op,
                    ty,
                    source_ty: (source != NO_SOURCE_TYPE).then(|| TypeId::new(source)),
                    args: self.elems.slice(at + 1, len as usize),
                }
            }
        }
    }

    /// The type of constant `id`.
    #[inline]
    pub fn ty(&self, id: ConstId) -> TypeId {
        match self.data[id.index()] {
            Repr::Int { ty, .. }
            | Repr::WideInt { ty, .. }
            | Repr::Float { ty, .. }
            | Repr::WideFloat { ty, .. }
            | Repr::Null(ty)
            | Repr::Zero(ty)
            | Repr::Undef(ty)
            | Repr::Poison(ty)
            | Repr::Aggregate { ty, .. }
            | Repr::Splat { ty, .. }
            | Repr::Bytes { ty, .. }
            | Repr::SymDiff { ty, .. }
            | Repr::IntOfAddr { ty, .. }
            | Repr::Expr { ty, .. } => ty,
            Repr::NoneToken => TypeId::TOKEN,
            Repr::Addr { .. }
            | Repr::BlockAddr { .. }
            | Repr::AddrOfInt { .. }
            | Repr::DsoLocalEquivalent(_)
            | Repr::NoCfi(_) => TypeId::PTR,
        }
    }

    /// The type of constant `id` if it is plain data with nothing inside
    /// it (a number, `null`, a zero, `undef`, `poison`, bytes); `None`
    /// for any other constant and for an id that names none. `classify`
    /// (§8) asks this of every constant operand: such a constant needs no
    /// look beyond its type.
    #[inline]
    pub(crate) fn leaf_type(&self, id: ConstId) -> Option<TypeId> {
        match *self.data.get(id.index())? {
            Repr::Int { ty, .. }
            | Repr::WideInt { ty, .. }
            | Repr::Float { ty, .. }
            | Repr::WideFloat { ty, .. }
            | Repr::Null(ty)
            | Repr::Zero(ty)
            | Repr::Undef(ty)
            | Repr::Poison(ty)
            | Repr::Bytes { ty, .. } => Some(ty),
            _ => None,
        }
    }

    /// Whether `id` is the zero of its type: integer zero, float `+0.0`,
    /// `null` or [`Const::Zero`]. Because all-zero aggregates are
    /// canonicalized, this is exact: no other constant is all zero bits
    /// without relocations. (`undef` and `poison` are not zero here.)
    pub fn is_zero(&self, id: ConstId) -> bool {
        match self.data[id.index()] {
            Repr::Int { value: 0, .. }
            | Repr::Float { bits: 0, .. }
            | Repr::Null(_)
            | Repr::Zero(_) => true,
            Repr::WideInt { words, len, .. } => {
                self.words.slice(words as usize, len as usize).iter().all(|&w| w == 0)
            }
            Repr::WideFloat { words, .. } => {
                self.words.slice(words as usize, 2).iter().all(|&w| w == 0)
            }
            _ => false,
        }
    }

    /// Whether `id` is zero, `undef` or `poison`, or an aggregate or vector
    /// of nothing but such constants: its image is all zero bytes with no
    /// relocations. This is LLVM's test for placing a writable variable in
    /// `.bss` (`isNullOrUndef` in `TargetLoweringObjectFile`).
    pub fn is_zero_or_undef(&self, id: ConstId) -> bool {
        match self.data[id.index()] {
            Repr::Undef(_) | Repr::Poison(_) => true,
            Repr::Aggregate { elems, len, .. } => self
                .elems
                .slice(elems as usize, len as usize)
                .iter()
                .all(|&elem| self.is_zero_or_undef(elem)),
            Repr::Splat { elem, .. } => self.is_zero_or_undef(elem),
            _ => self.is_zero(id),
        }
    }

    /// The value of an integer constant at most 64 bits wide, zero-extended.
    pub fn int_value(&self, id: ConstId) -> Option<u64> {
        match self.data[id.index()] {
            Repr::Int { value, .. } => Some(value),
            _ => None,
        }
    }

    /// The words of an integer constant of any width, in canonical form.
    pub(crate) fn int_words(&self, id: ConstId) -> Option<Vec<u64>> {
        match self.get(id) {
            Const::Int { value, .. } => Some(vec![value]),
            Const::WideInt { words, .. } => Some(words.to_vec()),
            _ => None,
        }
    }

    // ---- interning ----

    /// Interns an entry with no pooled part.
    fn intern(&mut self, repr: Repr) -> IrResult<ConstId> {
        let hash = inline_hash(&repr);
        if let Some(id) = self.interner.find(hash, |id| self.data[id as usize] == repr) {
            return Ok(ConstId::new(id));
        }
        let id = next_id(self.data.len(), "constant")?;
        self.data.push(repr);
        self.interner.insert(hash, id);
        Ok(ConstId::new(id))
    }

    /// Interns an entry with a pooled part: `same` recognizes an equal
    /// stored entry, `store` appends the pooled part and returns the entry.
    fn intern_pooled(
        &mut self,
        hash: u32,
        same: impl Fn(&Self, &Repr) -> bool,
        store: impl FnOnce(&mut Self) -> IrResult<Repr>,
    ) -> IrResult<ConstId> {
        if let Some(id) = self.interner.find(hash, |id| same(self, &self.data[id as usize])) {
            return Ok(ConstId::new(id));
        }
        let id = next_id(self.data.len(), "constant")?;
        let repr = store(self)?;
        self.data.push(repr);
        self.interner.insert(hash, id);
        Ok(ConstId::new(id))
    }

    fn intern_words(
        &mut self,
        seed: u64,
        ty: TypeId,
        extra: u32,
        words: &[u64],
        same: impl Fn(&Repr) -> Option<(u32, usize)>,
        make: impl FnOnce(u32) -> Repr,
    ) -> IrResult<ConstId> {
        let mut hasher = Hasher64::new(seed);
        hasher.pair(ty.raw(), extra);
        for &word in words {
            hasher.word(word);
        }
        self.intern_pooled(
            hasher.finish(),
            |pool, repr| {
                same(repr).is_some_and(|(at, len)| pool.words.slice(at as usize, len) == words)
            },
            |pool| {
                let start = pool_start(pool.words.len(), words.len(), "constant word")?;
                pool.words.extend_from_slice(words);
                Ok(make(start))
            },
        )
    }

    // ---- §4.1 data ----

    /// The integer of type `ty` whose value is `value` modulo `2^N`: the
    /// low `N` bits of `value`, and zero above bit 63 for a wider type.
    /// `TypeMismatch` unless `ty` is an integer type.
    pub fn int(&mut self, types: &TypeTable<L>, ty: TypeId, value: u64) -> IrResult<ConstId> {
        let bits = int_width(types, ty)?;
        if bits <= 64 {
            return self.intern(Repr::Int { ty, value: value & wide::mask(bits) });
        }
        self.int_from_words(types, ty, &[value])
    }

    /// The integer of type `ty` whose value is `value` modulo `2^N`, with
    /// `value` sign-extended first: `-1` is all ones at any width.
    pub fn int_signed(
        &mut self,
        types: &TypeTable<L>,
        ty: TypeId,
        value: i64,
    ) -> IrResult<ConstId> {
        let bits = int_width(types, ty)?;
        if bits <= 64 {
            return self.intern(Repr::Int { ty, value: value as u64 & wide::mask(bits) });
        }
        let mut words = vec![value as u64];
        wide::sign_extend(&mut words, 64, bits);
        self.int_from_words(types, ty, &words)
    }

    /// The integer of type `ty` whose value is the little-endian `words`
    /// modulo `2^N`. Missing high words are zero; excess bits are dropped.
    pub fn int_from_words(
        &mut self,
        types: &TypeTable<L>,
        ty: TypeId,
        words: &[u64],
    ) -> IrResult<ConstId> {
        let bits = int_width(types, ty)?;
        let mut canonical = words.to_vec();
        wide::truncate(&mut canonical, bits);
        if bits <= 64 {
            return self.intern(Repr::Int { ty, value: canonical[0] });
        }
        let len = canonical.len() as u32;
        self.intern_words(
            seed::WIDE_INT,
            ty,
            0,
            &canonical,
            |repr| match *repr {
                Repr::WideInt { ty: t, words, len } if t == ty => Some((words, len as usize)),
                _ => None,
            },
            |words| Repr::WideInt { ty, words, len },
        )
    }

    /// The floating-point constant of type `ty` with encoding `bits`.
    /// `TypeMismatch` unless `ty` is a floating-point type;
    /// `InvalidOperand` when `bits` has bits set above the type's width.
    pub fn float(&mut self, types: &TypeTable<L>, ty: TypeId, bits: u128) -> IrResult<ConstId> {
        if !types.is_float(ty) {
            return err(IrErrorKind::TypeMismatch, "a float constant needs a floating-point type");
        }
        let width = types.size_in_bits_of(ty) as u32;
        if width < 128 && bits >> width != 0 {
            return err(IrErrorKind::InvalidOperand, "float bit pattern is wider than its type");
        }
        if width <= 64 {
            return self.intern(Repr::Float { ty, bits: bits as u64 });
        }
        self.intern_words(
            seed::WIDE_FLOAT,
            ty,
            0,
            &[bits as u64, (bits >> 64) as u64],
            |repr| match *repr {
                Repr::WideFloat { ty: t, words } if t == ty => Some((words, 2)),
                _ => None,
            },
            |words| Repr::WideFloat { ty, words },
        )
    }

    /// The `float` constant `value`.
    pub fn f32(&mut self, value: f32) -> IrResult<ConstId> {
        self.intern(Repr::Float { ty: TypeId::FLOAT, bits: u64::from(value.to_bits()) })
    }

    /// The `double` constant `value`.
    pub fn f64(&mut self, value: f64) -> IrResult<ConstId> {
        self.intern(Repr::Float { ty: TypeId::DOUBLE, bits: value.to_bits() })
    }

    /// `null` of pointer type `ty`. `TypeMismatch` for any other type.
    pub fn null(&mut self, types: &TypeTable<L>, ty: TypeId) -> IrResult<ConstId> {
        if !types.is_ptr(ty) {
            return err(IrErrorKind::TypeMismatch, "null needs a pointer type");
        }
        self.intern(Repr::Null(ty))
    }

    /// The zero of `ty` (`zeroinitializer`): integer zero, `+0.0`, `null`,
    /// or [`Const::Zero`] for an aggregate, vector or target extension
    /// type. `InvalidType` for a type with no values.
    pub fn zero(&mut self, types: &TypeTable<L>, ty: TypeId) -> IrResult<ConstId> {
        match types.kind(ty) {
            TypeKind::Int(_) => self.int(types, ty, 0),
            TypeKind::Ptr { .. } => self.intern(Repr::Null(ty)),
            TypeKind::Array { .. }
            | TypeKind::Struct { .. }
            | TypeKind::Vector { .. }
            | TypeKind::ScalableVector { .. }
            | TypeKind::TargetExt { .. } => self.intern(Repr::Zero(ty)),
            _ if types.is_float(ty) => self.float(types, ty, 0),
            _ => err(IrErrorKind::InvalidType, "this type has no zero value"),
        }
    }

    /// `undef` of `ty`. `InvalidType` for `void`, `label`, `metadata`, a
    /// function type or an opaque struct.
    pub fn undef(&mut self, types: &TypeTable<L>, ty: TypeId) -> IrResult<ConstId> {
        check_first_class(types, ty)?;
        self.intern(Repr::Undef(ty))
    }

    /// `poison` of `ty`. Errors as [`Self::undef`].
    pub fn poison(&mut self, types: &TypeTable<L>, ty: TypeId) -> IrResult<ConstId> {
        check_first_class(types, ty)?;
        self.intern(Repr::Poison(ty))
    }

    /// `none`, the constant of type `token`.
    pub fn none_token(&mut self) -> IrResult<ConstId> {
        self.intern(Repr::NoneToken)
    }

    /// The array, struct or fixed vector of type `ty` with these elements.
    ///
    /// `TypeMismatch` when `ty` is not such a type or an element's type is
    /// not the element or field type; `InvalidOperand` when the number of
    /// elements is not the type's. The result is canonical: see the module
    /// documentation.
    pub fn aggregate(
        &mut self,
        types: &TypeTable<L>,
        ty: TypeId,
        elems: &[ConstId],
    ) -> IrResult<ConstId> {
        let count = elems.len() as u64;
        let (expected, is_vector, byte_array) = match types.kind(ty) {
            TypeKind::Struct { fields, .. } => {
                if fields.len() as u64 == count
                    && let Some(at) = (0..elems.len()).find(|&i| self.ty(elems[i]) != fields[i])
                {
                    return err(
                        IrErrorKind::TypeMismatch,
                        format!("field {at} has the wrong type"),
                    );
                }
                (fields.len() as u64, false, false)
            }
            TypeKind::Array { elem, len } => {
                self.check_elements(elems, elem)?;
                (len, false, elem == TypeId::I8)
            }
            TypeKind::Vector { elem, len } => {
                self.check_elements(elems, elem)?;
                (u64::from(len), true, false)
            }
            TypeKind::ScalableVector { .. } => {
                return err(
                    IrErrorKind::TypeMismatch,
                    "a scalable vector constant is a splat, not an element list",
                );
            }
            _ => return err(IrErrorKind::TypeMismatch, "not an aggregate or vector type"),
        };
        if expected != count {
            return err(
                IrErrorKind::InvalidOperand,
                format!("{count} elements for a type with {expected}"),
            );
        }
        if elems.iter().all(|&e| self.is_zero(e)) {
            return self.intern(Repr::Zero(ty));
        }
        if elems.iter().all(|&e| matches!(self.data[e.index()], Repr::Poison(_))) {
            return self.intern(Repr::Poison(ty));
        }
        if elems.iter().all(|&e| matches!(self.data[e.index()], Repr::Undef(_))) {
            return self.intern(Repr::Undef(ty));
        }
        if byte_array && let Some(bytes) = self.byte_values(elems) {
            return self.bytes_of_type(ty, &bytes);
        }
        if is_vector && elems.iter().all(|&e| e == elems[0]) {
            return self.intern(Repr::Splat { ty, elem: elems[0] });
        }
        self.intern_aggregate(ty, elems)
    }

    /// The stored aggregate of type `ty` with these elements, canonical
    /// already.
    fn intern_aggregate(&mut self, ty: TypeId, elems: &[ConstId]) -> IrResult<ConstId> {
        let mut hasher = Hasher64::new(seed::AGGREGATE);
        hasher.pair(ty.raw(), elems.len() as u32);
        for elem in elems {
            hasher.word(u64::from(elem.raw()));
        }
        self.intern_pooled(
            hasher.finish(),
            |pool, repr| match *repr {
                Repr::Aggregate { ty: t, elems: at, len } => {
                    t == ty && pool.elems.slice(at as usize, len as usize) == elems
                }
                _ => false,
            },
            |pool| {
                let start = pool_start(pool.elems.len(), elems.len(), "constant element")?;
                pool.elems.extend_from_slice(elems);
                Ok(Repr::Aggregate { ty, elems: start, len: elems.len() as u32 })
            },
        )
    }

    fn check_elements(&self, elems: &[ConstId], elem_ty: TypeId) -> IrResult<()> {
        match elems.iter().position(|&e| self.ty(e) != elem_ty) {
            Some(at) => err(IrErrorKind::TypeMismatch, format!("element {at} has the wrong type")),
            None => Ok(()),
        }
    }

    /// The values of `elems` when every one is an `i8` integer.
    fn byte_values(&self, elems: &[ConstId]) -> Option<Vec<u8>> {
        elems
            .iter()
            .map(|&e| match self.data[e.index()] {
                Repr::Int { value, .. } => Some(value as u8),
                _ => None,
            })
            .collect()
    }

    /// The vector of type `ty`, fixed or scalable, whose every element is
    /// `elem`. `TypeMismatch` unless `ty` is a vector of `elem`'s type.
    pub fn splat(&mut self, types: &TypeTable<L>, ty: TypeId, elem: ConstId) -> IrResult<ConstId> {
        let (TypeKind::Vector { elem: elem_ty, .. }
        | TypeKind::ScalableVector { elem: elem_ty, .. }) = types.kind(ty)
        else {
            return err(IrErrorKind::TypeMismatch, "a splat needs a vector type");
        };
        if self.ty(elem) != elem_ty {
            return err(IrErrorKind::TypeMismatch, "splat element has the wrong type");
        }
        let repr = match self.data[elem.index()] {
            _ if self.is_zero(elem) => Repr::Zero(ty),
            Repr::Poison(_) => Repr::Poison(ty),
            Repr::Undef(_) => Repr::Undef(ty),
            _ => Repr::Splat { ty, elem },
        };
        self.intern(repr)
    }

    /// The `[N x i8]` constant with contents `data` (`c"..."`). The array
    /// type is interned in `types`.
    pub fn bytes(&mut self, types: &mut TypeTable<L>, data: &[u8]) -> IrResult<ConstId> {
        let ty = types.array(TypeId::I8, data.len() as u64)?;
        self.bytes_of_type(ty, data)
    }

    /// `data` as a constant of `ty`, which is `[data.len() x i8]`.
    fn bytes_of_type(&mut self, ty: TypeId, data: &[u8]) -> IrResult<ConstId> {
        if data.iter().all(|&b| b == 0) {
            return self.intern(Repr::Zero(ty));
        }
        let mut hasher = Hasher64::new(seed::BYTES);
        hasher.bytes(data);
        self.intern_pooled(
            hasher.finish(),
            |pool, repr| match *repr {
                Repr::Bytes { start, len, .. } => {
                    pool.bytes.slice(start as usize, len as usize) == data
                }
                _ => false,
            },
            |pool| {
                let start = pool_start(pool.bytes.len(), data.len(), "constant byte")?;
                pool.bytes.extend_from_slice(data);
                Ok(Repr::Bytes { ty, start, len: data.len() as u32 })
            },
        )
    }

    // ---- §4.2 addresses ----

    /// The address of `sym` plus `addend` bytes.
    pub fn addr(&mut self, sym: SymId, addend: i64) -> IrResult<ConstId> {
        self.intern(Repr::Addr { sym, addend })
    }

    /// `blockaddress(func, block)`.
    pub fn block_addr(&mut self, func: SymId, block: BlockId) -> IrResult<ConstId> {
        let before = self.data.len();
        let id = self.intern(Repr::BlockAddr { func, block })?;
        if self.data.len() != before {
            self.block_addrs.push(id);
        }
        Ok(id)
    }

    /// Every `blockaddress` constant of the pool, in id order (WP-064):
    /// the blocks whose address is taken, anywhere in the module.
    pub fn block_addresses(&self) -> impl Iterator<Item = (ConstId, SymId, BlockId)> + '_ {
        self.block_addrs.iter().filter_map(|&id| match self.data[id.index()] {
            Repr::BlockAddr { func, block } => Some((id, func, block)),
            _ => None,
        })
    }

    /// Whether `blockaddress(func, block)` is a constant of the pool:
    /// the block's address is taken (§4.2, WP-064).
    pub fn is_address_taken(&self, func: SymId, block: BlockId) -> bool {
        self.block_addresses().any(|(_, f, b)| f == func && b == block)
    }

    /// Whether a block of `func` has its address taken.
    pub fn has_address_taken_block(&self, func: SymId) -> bool {
        self.block_addresses().any(|(_, f, _)| f == func)
    }

    /// `dso_local_equivalent @sym`.
    pub fn dso_local_equivalent(&mut self, sym: SymId) -> IrResult<ConstId> {
        self.intern(Repr::DsoLocalEquivalent(sym))
    }

    /// `no_cfi @sym`.
    pub fn no_cfi(&mut self, sym: SymId) -> IrResult<ConstId> {
        self.intern(Repr::NoCfi(sym))
    }

    /// `a - b + addend` as an integer of type `ty`, where `plt` says that
    /// `a` is `dso_local_equivalent @a`.
    ///
    /// `TypeMismatch` unless `ty` is an integer type of at most 64 bits.
    /// The addend is reduced to the type's width, and the difference of a
    /// symbol with itself is the integer `addend`.
    pub fn sym_diff(
        &mut self,
        types: &TypeTable<L>,
        ty: TypeId,
        a: SymId,
        b: SymId,
        addend: i64,
        plt: bool,
    ) -> IrResult<ConstId> {
        let bits = int_width(types, ty)?;
        if bits > 64 {
            return err(IrErrorKind::TypeMismatch, "a symbol difference is at most 64 bits wide");
        }
        if a == b && !plt {
            return self.int(types, ty, addend as u64);
        }
        let addend = wide::sext64(addend as u64 & wide::mask(bits), bits);
        let words = [u64::from(a.raw()) | (u64::from(b.raw()) << 32), addend as u64];
        self.intern_words(
            seed::SYM_DIFF,
            ty,
            u32::from(plt),
            &words,
            |repr| match *repr {
                Repr::SymDiff { ty: t, words, plt: p } if t == ty && p == plt => Some((words, 2)),
                _ => None,
            },
            |words| Repr::SymDiff { ty, words, plt },
        )
    }

    /// `ptrtoint` of the address constant `addr` to integer type `ty`.
    ///
    /// `TypeMismatch` unless `ty` is an integer type; `InvalidOperand`
    /// unless `addr` is an `Addr`, `BlockAddr`, `DsoLocalEquivalent` or
    /// `NoCfi`. (`ConstPool::fold_cast` also reduces the other pointer
    /// constants.)
    ///
    /// For a type narrower than 64 bits only the low bits of an `Addr`'s
    /// addend matter, so the addend is first reduced to the type's width
    /// (sign-extended): the result may wrap a different `Addr` than the one
    /// given.
    pub fn int_of_addr(
        &mut self,
        types: &TypeTable<L>,
        ty: TypeId,
        addr: ConstId,
    ) -> IrResult<ConstId> {
        let bits = int_width(types, ty)?;
        if !self.is_symbolic_address(addr) {
            return err(IrErrorKind::InvalidOperand, "not an address constant");
        }
        let mut addr = addr;
        if bits < 64
            && let Repr::Addr { sym, addend } = self.data[addr.index()]
        {
            let reduced = wide::sext64(addend as u64 & wide::mask(bits), bits);
            if reduced != addend {
                addr = self.addr(sym, reduced)?;
            }
        }
        self.intern(Repr::IntOfAddr { ty, addr })
    }

    /// The absolute address `value` as a `ptr`: `inttoptr (i64 value)`.
    /// Address zero is `null`.
    pub fn addr_of_int(&mut self, value: u64) -> IrResult<ConstId> {
        if value == 0 {
            return self.intern(Repr::Null(TypeId::PTR));
        }
        self.intern(Repr::AddrOfInt { value })
    }

    /// Whether `id` is an address that needs a relocation: an `Addr`,
    /// `BlockAddr`, `DsoLocalEquivalent` or `NoCfi`.
    pub fn is_symbolic_address(&self, id: ConstId) -> bool {
        matches!(
            self.data[id.index()],
            Repr::Addr { .. }
                | Repr::BlockAddr { .. }
                | Repr::DsoLocalEquivalent(_)
                | Repr::NoCfi(_)
        )
    }

    /// The constant expression `op` over `args` with result type `ty`, kept
    /// unreduced. `source_ty` is the source element type of a
    /// `getelementptr`.
    ///
    /// Producers call the `fold_*` functions instead, which return an
    /// `Expr` only when no normal form applies; this is for the operators
    /// that have no fold (`extractelement` and the like).
    pub fn expr(
        &mut self,
        op: ConstOp,
        ty: TypeId,
        source_ty: Option<TypeId>,
        args: &[ConstId],
    ) -> IrResult<ConstId> {
        let source = source_ty.map_or(NO_SOURCE_TYPE, TypeId::raw);
        let mut hasher = Hasher64::new(seed::EXPR);
        hasher.pair(ty.raw(), source);
        hasher.pair(op as u32, args.len() as u32);
        for arg in args {
            hasher.word(u64::from(arg.raw()));
        }
        self.intern_pooled(
            hasher.finish(),
            |pool, repr| match *repr {
                Repr::Expr { ty: t, op: o, args: at, len } => {
                    let at = at as usize;
                    t == ty
                        && o == op
                        && pool.elems[at].raw() == source
                        && pool.elems.slice(at + 1, len as usize) == args
                }
                _ => false,
            },
            |pool| {
                let start = pool_start(pool.elems.len(), args.len() + 1, "constant element")?;
                pool.elems.push(ConstId::new(source));
                pool.elems.extend_from_slice(args);
                Ok(Repr::Expr { ty, op, args: start, len: args.len() as u32 })
            },
        )
    }
}

/// The width of integer type `ty`, or `TypeMismatch`.
fn int_width<L: Layering>(types: &TypeTable<L>, ty: TypeId) -> IrResult<u32> {
    match types.int_bits(ty) {
        Some(bits) => Ok(bits),
        None => err(IrErrorKind::TypeMismatch, "an integer type is required"),
    }
}

fn check_first_class<L: Layering>(types: &TypeTable<L>, ty: TypeId) -> IrResult<()> {
    match types.kind(ty) {
        TypeKind::Void
        | TypeKind::Label
        | TypeKind::Metadata
        | TypeKind::Function { .. }
        | TypeKind::Opaque(_) => err(IrErrorKind::InvalidType, "this type has no values"),
        _ => Ok(()),
    }
}

/// The hash of an entry with no pooled part.
fn inline_hash(repr: &Repr) -> u32 {
    let (tag, a, b): (u64, u32, u64) = match *repr {
        Repr::Int { ty, value } => (1, ty.raw(), value),
        Repr::Float { ty, bits } => (2, ty.raw(), bits),
        Repr::Null(ty) => (3, ty.raw(), 0),
        Repr::Zero(ty) => (4, ty.raw(), 0),
        Repr::Undef(ty) => (5, ty.raw(), 0),
        Repr::Poison(ty) => (6, ty.raw(), 0),
        Repr::NoneToken => (7, 0, 0),
        Repr::Splat { ty, elem } => (8, ty.raw(), u64::from(elem.raw())),
        Repr::Addr { sym, addend } => (9, sym.raw(), addend as u64),
        Repr::BlockAddr { func, block } => (10, func.raw(), u64::from(block.raw())),
        Repr::IntOfAddr { ty, addr } => (11, ty.raw(), u64::from(addr.raw())),
        Repr::AddrOfInt { value } => (12, 0, value),
        Repr::DsoLocalEquivalent(sym) => (13, sym.raw(), 0),
        Repr::NoCfi(sym) => (14, sym.raw(), 0),
        Repr::WideInt { .. }
        | Repr::WideFloat { .. }
        | Repr::Aggregate { .. }
        | Repr::Bytes { .. }
        | Repr::SymDiff { .. }
        | Repr::Expr { .. } => unreachable!("entries with pooled parts are hashed by content"),
    };
    let mut hasher = Hasher64::new(tag);
    hasher.word(u64::from(a));
    hasher.word(b);
    hasher.finish()
}

#[cfg(test)]
// The tests name the two tables `t` and `c` and symbols `f` and `g`.
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;
    use crate::module::Module;
    use crate::target::TargetSpec;

    fn module() -> Module {
        Module::new(TargetSpec::aarch64_linux_gnu())
    }

    fn kind_of(result: IrResult<ConstId>) -> IrErrorKind {
        result.unwrap_err().kind
    }

    /// §4: a stored constant is 16 bytes.
    #[test]
    fn constant_entries_are_16_bytes() {
        assert_eq!(size_of::<Repr>(), 16);
        assert_eq!(size_of::<Const<'_>>(), 32);
        assert_eq!(size_of::<ConstOp>(), 1);
    }

    /// §4.1: integers are their low N bits, however they were given.
    #[test]
    fn integer_canonicalization() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let minus_one = c.int_signed(t, TypeId::I8, -1).unwrap();
        assert_eq!(c.int(t, TypeId::I8, 255).unwrap(), minus_one);
        assert_eq!(c.int(t, TypeId::I8, 0xffff_ffff_ffff_ffff).unwrap(), minus_one);
        assert_eq!(c.int(t, TypeId::I8, 0x1ff).unwrap(), minus_one);
        assert_eq!(c.int_from_words(t, TypeId::I8, &[0xff, 7, 9]).unwrap(), minus_one);
        assert_eq!(c.get(minus_one), Const::Int { ty: TypeId::I8, value: 255 });
        assert_eq!(c.int_value(minus_one), Some(255));
        assert_eq!(c.ty(minus_one), TypeId::I8);
        // The same value at another type is another constant.
        assert_ne!(c.int(t, TypeId::I16, 255).unwrap(), minus_one);
        let i1_true = c.int(t, TypeId::I1, 1).unwrap();
        assert_eq!(c.int_signed(t, TypeId::I1, -1).unwrap(), i1_true);
        assert_eq!(c.int(t, TypeId::I1, 3).unwrap(), i1_true);
        let i3 = t.int(3).unwrap();
        let seven = c.int_signed(t, i3, -1).unwrap();
        assert_eq!(c.get(seven), Const::Int { ty: i3, value: 7 });
        let max = c.int(t, TypeId::I64, u64::MAX).unwrap();
        assert_eq!(c.int_signed(t, TypeId::I64, -1).unwrap(), max);
        assert_eq!(
            c.int_from_words(t, TypeId::I64, &[]).unwrap(),
            c.int(t, TypeId::I64, 0).unwrap()
        );
        assert_eq!(kind_of(c.int(t, TypeId::FLOAT, 1)), IrErrorKind::TypeMismatch);
        assert_eq!(kind_of(c.int_signed(t, TypeId::PTR, 1)), IrErrorKind::TypeMismatch);
        assert_eq!(kind_of(c.int_from_words(t, TypeId::VOID, &[1])), IrErrorKind::TypeMismatch);
    }

    /// §4.1: wide integers are `ceil(N/64)` words with the top bits clear,
    /// so equal values intern to one id.
    #[test]
    fn wide_integer_canonicalization() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let five = c.int(t, TypeId::I128, 5).unwrap();
        assert_eq!(c.get(five), Const::WideInt { ty: TypeId::I128, words: &[5, 0] });
        assert_eq!(c.int_signed(t, TypeId::I128, 5).unwrap(), five);
        assert_eq!(c.int_from_words(t, TypeId::I128, &[5]).unwrap(), five);
        assert_eq!(c.int_from_words(t, TypeId::I128, &[5, 0, 0]).unwrap(), five);
        assert_eq!(c.int_from_words(t, TypeId::I128, &[5, 0, 99]).unwrap(), five);
        assert_eq!(c.int_value(five), None);
        let minus_one = c.int_signed(t, TypeId::I128, -1).unwrap();
        assert_eq!(c.get(minus_one), Const::WideInt { ty: TypeId::I128, words: &[u64::MAX; 2] });
        assert_eq!(c.int_from_words(t, TypeId::I128, &[u64::MAX; 2]).unwrap(), minus_one);
        // Zero-extension, not sign-extension, of a `u64` argument.
        let low_ones = c.int(t, TypeId::I128, u64::MAX).unwrap();
        assert_eq!(c.get(low_ones), Const::WideInt { ty: TypeId::I128, words: &[u64::MAX, 0] });
        let i65 = t.int(65).unwrap();
        let top = c.int_signed(t, i65, -1).unwrap();
        assert_eq!(c.get(top), Const::WideInt { ty: i65, words: &[u64::MAX, 1] });
        assert_eq!(c.int_from_words(t, i65, &[u64::MAX, u64::MAX]).unwrap(), top);
        assert_eq!(c.int_from_words(t, i65, &[u64::MAX, 0xff01, 3]).unwrap(), top);
        let i200 = t.int(200).unwrap();
        let big = c.int_signed(t, i200, -2).unwrap();
        assert_eq!(
            c.get(big),
            Const::WideInt { ty: i200, words: &[u64::MAX - 1, u64::MAX, u64::MAX, 0xff] }
        );
        assert_ne!(c.int(t, i65, 5).unwrap(), five);
        assert_eq!(c.ty(big), i200);
        let before = c.len();
        c.int(t, TypeId::I128, 5).unwrap();
        c.int_signed(t, i200, -2).unwrap();
        assert_eq!(c.len(), before);
    }

    /// §4.1: floats are bit patterns.
    #[test]
    fn floats_intern_by_bit_pattern() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let one = c.f64(1.0).unwrap();
        assert_eq!(c.float(t, TypeId::DOUBLE, u128::from(1.0f64.to_bits())).unwrap(), one);
        assert_eq!(c.get(one), Const::Float { ty: TypeId::DOUBLE, bits: 0x3ff0_0000_0000_0000 });
        assert_ne!(c.f64(0.0).unwrap(), c.f64(-0.0).unwrap());
        assert_ne!(c.f32(1.0).unwrap(), one);
        let single = c.f32(1.0).unwrap();
        assert_eq!(c.get(single), Const::Float { ty: TypeId::FLOAT, bits: 0x3f80_0000 });
        let nan_a = c.float(t, TypeId::DOUBLE, 0x7ff8_0000_0000_0000).unwrap();
        let nan_b = c.float(t, TypeId::DOUBLE, 0x7ff8_0000_0000_0001).unwrap();
        assert_ne!(nan_a, nan_b);
        let half = c.float(t, TypeId::HALF, 0x3c00).unwrap();
        assert_ne!(c.float(t, TypeId::BFLOAT, 0x3c00).unwrap(), half);
        let quad = c.float(t, TypeId::FP128, 0x3fff << 112).unwrap();
        assert_eq!(c.get(quad), Const::WideFloat { ty: TypeId::FP128, bits: 0x3fff << 112 });
        assert_eq!(c.float(t, TypeId::FP128, 0x3fff << 112).unwrap(), quad);
        let x87 = c.float(t, TypeId::X86_FP80, 0x3fff_8000_0000_0000_0000).unwrap();
        assert_eq!(
            c.get(x87),
            Const::WideFloat { ty: TypeId::X86_FP80, bits: 0x3fff_8000_0000_0000_0000 }
        );
        assert_ne!(c.float(t, TypeId::FP128, 0x3fff_8000_0000_0000_0000).unwrap(), x87);
        assert_eq!(c.ty(x87), TypeId::X86_FP80);
        assert_eq!(kind_of(c.float(t, TypeId::HALF, 0x1_0000)), IrErrorKind::InvalidOperand);
        assert_eq!(kind_of(c.float(t, TypeId::FLOAT, 1 << 32)), IrErrorKind::InvalidOperand);
        assert_eq!(kind_of(c.float(t, TypeId::X86_FP80, 1 << 80)), IrErrorKind::InvalidOperand);
        assert_eq!(kind_of(c.float(t, TypeId::I32, 0)), IrErrorKind::TypeMismatch);
        c.float(t, TypeId::FP128, u128::MAX).unwrap();
    }

    /// §4.1: every zero has one form, and all-zero aggregates reduce to it.
    #[test]
    fn zero_canonicalization() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        assert_eq!(c.zero(t, TypeId::I32).unwrap(), c.int(t, TypeId::I32, 0).unwrap());
        assert_eq!(c.zero(t, TypeId::I128).unwrap(), c.int(t, TypeId::I128, 0).unwrap());
        assert_eq!(c.zero(t, TypeId::DOUBLE).unwrap(), c.f64(0.0).unwrap());
        assert_eq!(c.zero(t, TypeId::FP128).unwrap(), c.float(t, TypeId::FP128, 0).unwrap());
        assert_eq!(c.zero(t, TypeId::PTR).unwrap(), c.null(t, TypeId::PTR).unwrap());
        assert_eq!(c.addr_of_int(0).unwrap(), c.null(t, TypeId::PTR).unwrap());
        let st = t
            .struct_type(&[TypeId::I32, TypeId::PTR, TypeId::DOUBLE, TypeId::I128], false)
            .unwrap();
        let zero = c.zero(t, st).unwrap();
        assert_eq!(c.get(zero), Const::Zero(st));
        let fields = [
            c.int(t, TypeId::I32, 0).unwrap(),
            c.null(t, TypeId::PTR).unwrap(),
            c.f64(0.0).unwrap(),
            c.int(t, TypeId::I128, 0).unwrap(),
        ];
        assert_eq!(c.aggregate(t, st, &fields).unwrap(), zero);
        // Nested: an array of all-zero structs is the array's zero.
        let arr = t.array(st, 3).unwrap();
        assert_eq!(c.aggregate(t, arr, &[zero; 3]).unwrap(), c.zero(t, arr).unwrap());
        // `-0.0` is not zero.
        let mut with_neg = fields;
        with_neg[2] = c.f64(-0.0).unwrap();
        let not_zero = c.aggregate(t, st, &with_neg).unwrap();
        assert_ne!(not_zero, zero);
        assert!(!c.is_zero(not_zero) && c.is_zero(zero));
        // Byte strings, splats, empty aggregates.
        let zeros = c.bytes(t, &[0; 6]).unwrap();
        assert_eq!(c.get(zeros), Const::Zero(t.array(TypeId::I8, 6).unwrap()));
        let empty_bytes = c.bytes(t, &[]).unwrap();
        assert_eq!(c.get(empty_bytes), Const::Zero(t.array(TypeId::I8, 0).unwrap()));
        let vec = t.vector(TypeId::I32, 4).unwrap();
        let int_zero = c.int(t, TypeId::I32, 0).unwrap();
        assert_eq!(c.splat(t, vec, int_zero).unwrap(), c.zero(t, vec).unwrap());
        assert_eq!(c.aggregate(t, vec, &[int_zero; 4]).unwrap(), c.zero(t, vec).unwrap());
        let empty = t.struct_type(&[], false).unwrap();
        assert_eq!(c.aggregate(t, empty, &[]).unwrap(), c.zero(t, empty).unwrap());
        let scalable = t.scalable_vector(TypeId::I32, 4).unwrap();
        assert_eq!(c.splat(t, scalable, int_zero).unwrap(), c.zero(t, scalable).unwrap());
        let ext = t.target_ext("x").unwrap();
        let ext_zero = c.zero(t, ext).unwrap();
        assert_eq!(c.get(ext_zero), Const::Zero(ext));
        for id in [zero, int_zero, zeros, c.f64(0.0).unwrap(), c.null(t, TypeId::PTR).unwrap()] {
            assert!(c.is_zero(id));
        }
        let i128_zero = c.int(t, TypeId::I128, 0).unwrap();
        let fp128_zero = c.float(t, TypeId::FP128, 0).unwrap();
        assert!(c.is_zero(i128_zero) && c.is_zero(fp128_zero));
        let one = c.int(t, TypeId::I128, 1).unwrap();
        let undef = c.undef(t, TypeId::I32).unwrap();
        assert!(!c.is_zero(one) && !c.is_zero(undef));
        let opaque = t.opaque_struct().unwrap();
        for ty in [TypeId::VOID, TypeId::LABEL, TypeId::METADATA, TypeId::TOKEN, opaque] {
            assert_eq!(kind_of(c.zero(t, ty)), IrErrorKind::InvalidType);
        }
        assert_eq!(kind_of(c.null(t, TypeId::I64)), IrErrorKind::TypeMismatch);
    }

    /// §4.1: `undef` and `poison` are distinct constants, and aggregates of
    /// only one of them reduce to it.
    #[test]
    fn undef_and_poison() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let undef = c.undef(t, TypeId::I32).unwrap();
        let poison = c.poison(t, TypeId::I32).unwrap();
        assert_ne!(undef, poison);
        assert_eq!(c.undef(t, TypeId::I32).unwrap(), undef);
        assert_eq!(c.get(undef), Const::Undef(TypeId::I32));
        assert_eq!(c.get(poison), Const::Poison(TypeId::I32));
        let arr = t.array(TypeId::I32, 2).unwrap();
        assert_eq!(c.aggregate(t, arr, &[undef, undef]).unwrap(), c.undef(t, arr).unwrap());
        assert_eq!(c.aggregate(t, arr, &[poison, poison]).unwrap(), c.poison(t, arr).unwrap());
        let mixed = c.aggregate(t, arr, &[undef, poison]).unwrap();
        assert_eq!(c.get(mixed), Const::Aggregate { ty: arr, elems: &[undef, poison] });
        // §4.3: what LLVM places in `.bss`.
        let zero = c.int(t, TypeId::I32, 0).unwrap();
        let one = c.int(t, TypeId::I32, 1).unwrap();
        let half_undef = c.aggregate(t, arr, &[zero, undef]).unwrap();
        let nested_ty = t.array(arr, 2).unwrap();
        let nested = c.aggregate(t, nested_ty, &[half_undef, mixed]).unwrap();
        let not_zero = c.aggregate(t, arr, &[one, undef]).unwrap();
        let minus_zero = c.f64(-0.0).unwrap();
        for id in [undef, poison, zero, mixed, half_undef, nested] {
            assert!(c.is_zero_or_undef(id));
        }
        for id in [one, not_zero, minus_zero] {
            assert!(!c.is_zero_or_undef(id));
        }
        assert!(!c.is_zero(half_undef) && !c.is_zero(undef));
        let vec = t.vector(TypeId::I32, 2).unwrap();
        assert_eq!(c.splat(t, vec, poison).unwrap(), c.poison(t, vec).unwrap());
        assert_eq!(c.splat(t, vec, undef).unwrap(), c.undef(t, vec).unwrap());
        let token = c.none_token().unwrap();
        assert_eq!(c.none_token().unwrap(), token);
        assert_eq!(c.get(token), Const::NoneToken);
        assert_eq!(c.ty(token), TypeId::TOKEN);
        let func = t.function(TypeId::VOID, &[], false).unwrap();
        let opaque = t.opaque_struct().unwrap();
        for ty in [TypeId::VOID, TypeId::LABEL, TypeId::METADATA, func, opaque] {
            assert_eq!(kind_of(c.undef(t, ty)), IrErrorKind::InvalidType);
            assert_eq!(kind_of(c.poison(t, ty)), IrErrorKind::InvalidType);
        }
    }

    /// §4.1: aggregates intern by type and elements; `i8` arrays are byte
    /// strings; uniform vectors are splats.
    #[test]
    fn aggregate_identity_and_canonical_forms() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let one = c.int(t, TypeId::I32, 1).unwrap();
        let two = c.int(t, TypeId::I32, 2).unwrap();
        let arr = t.array(TypeId::I32, 2).unwrap();
        let st = t.struct_type(&[TypeId::I32, TypeId::I32], false).unwrap();
        let vec = t.vector(TypeId::I32, 2).unwrap();
        let a = c.aggregate(t, arr, &[one, two]).unwrap();
        assert_eq!(c.aggregate(t, arr, &[one, two]).unwrap(), a);
        assert_ne!(c.aggregate(t, arr, &[two, one]).unwrap(), a);
        // Same elements, different type: different constants.
        let s = c.aggregate(t, st, &[one, two]).unwrap();
        let v = c.aggregate(t, vec, &[one, two]).unwrap();
        assert!(a != s && a != v && s != v);
        assert_eq!(c.get(s), Const::Aggregate { ty: st, elems: &[one, two] });
        assert_eq!(c.ty(v), vec);
        // A uniform fixed vector is a splat; a uniform array is not.
        let splat = c.splat(t, vec, one).unwrap();
        assert_eq!(c.aggregate(t, vec, &[one, one]).unwrap(), splat);
        assert_eq!(c.get(splat), Const::Splat { ty: vec, elem: one });
        let uniform = c.aggregate(t, arr, &[one, one]).unwrap();
        assert_eq!(c.get(uniform), Const::Aggregate { ty: arr, elems: &[one, one] });
        let scalable = t.scalable_vector(TypeId::I32, 2).unwrap();
        let scalable_splat = c.splat(t, scalable, one).unwrap();
        assert_eq!(c.get(scalable_splat), Const::Splat { ty: scalable, elem: one });
        assert_ne!(scalable_splat, splat);
        // Byte strings.
        let hi = c.bytes(t, b"hi\0").unwrap();
        let bytes_ty = t.array(TypeId::I8, 3).unwrap();
        assert_eq!(c.get(hi), Const::Bytes { ty: bytes_ty, bytes: b"hi\0" });
        let chars = [
            c.int(t, TypeId::I8, u64::from(b'h')).unwrap(),
            c.int(t, TypeId::I8, u64::from(b'i')).unwrap(),
            c.int(t, TypeId::I8, 0).unwrap(),
        ];
        assert_eq!(c.aggregate(t, bytes_ty, &chars).unwrap(), hi);
        assert_eq!(c.bytes(t, b"hi\0").unwrap(), hi);
        assert_ne!(c.bytes(t, b"hi").unwrap(), hi);
        assert_ne!(c.bytes(t, b"ho\0").unwrap(), hi);
        // An `i8` array with a non-integer element stays an element list.
        let undef8 = c.undef(t, TypeId::I8).unwrap();
        let partial = c.aggregate(t, bytes_ty, &[chars[0], undef8, chars[2]]).unwrap();
        assert_eq!(
            c.get(partial),
            Const::Aggregate { ty: bytes_ty, elems: &[chars[0], undef8, chars[2]] }
        );
        // A vector of i8 is not a byte string.
        let v8 = t.vector(TypeId::I8, 3).unwrap();
        let v8c = c.aggregate(t, v8, &chars).unwrap();
        assert_eq!(c.get(v8c), Const::Aggregate { ty: v8, elems: &chars });
    }

    /// §4.1 `TypeMismatch` and `InvalidOperand` from aggregate constructors.
    #[test]
    fn aggregate_errors() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let one = c.int(t, TypeId::I32, 1).unwrap();
        let wide = c.int(t, TypeId::I64, 1).unwrap();
        let arr = t.array(TypeId::I32, 2).unwrap();
        let st = t.struct_type(&[TypeId::I32, TypeId::I64], false).unwrap();
        let vec = t.vector(TypeId::I32, 2).unwrap();
        let scalable = t.scalable_vector(TypeId::I32, 2).unwrap();
        let before = c.len();
        let mismatch = IrErrorKind::TypeMismatch;
        let operand = IrErrorKind::InvalidOperand;
        assert_eq!(kind_of(c.aggregate(t, arr, &[one, wide])), mismatch);
        assert_eq!(kind_of(c.aggregate(t, st, &[one, one])), mismatch);
        assert_eq!(kind_of(c.aggregate(t, vec, &[wide, wide])), mismatch);
        assert_eq!(kind_of(c.aggregate(t, TypeId::I32, &[one])), mismatch);
        assert_eq!(kind_of(c.aggregate(t, scalable, &[one, one])), mismatch);
        assert_eq!(kind_of(c.aggregate(t, arr, &[one])), operand);
        assert_eq!(kind_of(c.aggregate(t, arr, &[one, one, one])), operand);
        assert_eq!(kind_of(c.aggregate(t, st, &[one])), operand);
        assert_eq!(kind_of(c.aggregate(t, st, &[one, wide, one])), operand);
        assert_eq!(kind_of(c.aggregate(t, vec, &[])), operand);
        assert_eq!(kind_of(c.splat(t, vec, wide)), mismatch);
        assert_eq!(kind_of(c.splat(t, arr, one)), mismatch);
        assert_eq!(c.len(), before);
        c.aggregate(t, st, &[one, wide]).unwrap();
    }

    /// §4.2: the address forms intern by value.
    #[test]
    fn address_forms() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let (f, g) = (SymId::new(1), SymId::new(2));
        let a = c.addr(g, 8).unwrap();
        assert_eq!(c.addr(g, 8).unwrap(), a);
        assert_ne!(c.addr(g, 0).unwrap(), a);
        assert_ne!(c.addr(f, 8).unwrap(), a);
        assert_eq!(c.get(a), Const::Addr { sym: g, addend: 8 });
        assert_eq!(c.ty(a), TypeId::PTR);
        let negative = c.addr(g, -8).unwrap();
        assert_eq!(c.get(negative), Const::Addr { sym: g, addend: -8 });
        let block = c.block_addr(f, BlockId::new(3)).unwrap();
        assert_eq!(c.block_addr(f, BlockId::new(3)).unwrap(), block);
        assert_ne!(c.block_addr(f, BlockId::new(4)).unwrap(), block);
        assert_eq!(c.get(block), Const::BlockAddr { func: f, block: BlockId::new(3) });
        let dso = c.dso_local_equivalent(f).unwrap();
        let no_cfi = c.no_cfi(f).unwrap();
        let plain = c.addr(f, 0).unwrap();
        assert!(dso != no_cfi && dso != plain && no_cfi != plain);
        assert_eq!(c.get(dso), Const::DsoLocalEquivalent(f));
        assert_eq!(c.get(no_cfi), Const::NoCfi(f));
        for id in [a, block, dso, no_cfi] {
            assert_eq!(c.ty(id), TypeId::PTR);
            assert!(c.is_symbolic_address(id));
        }
        let absolute = c.addr_of_int(0x1000).unwrap();
        assert_eq!(c.get(absolute), Const::AddrOfInt { value: 0x1000 });
        assert_eq!(c.ty(absolute), TypeId::PTR);
        assert!(!c.is_symbolic_address(absolute));

        let int = c.int_of_addr(t, TypeId::I64, a).unwrap();
        assert_eq!(c.int_of_addr(t, TypeId::I64, a).unwrap(), int);
        assert_ne!(c.int_of_addr(t, TypeId::I32, a).unwrap(), int);
        assert_eq!(c.get(int), Const::IntOfAddr { ty: TypeId::I64, addr: a });
        assert_eq!(c.ty(int), TypeId::I64);
        // Below 64 bits the addend is reduced to the width.
        let far = c.addr(g, (1 << 40) - 4).unwrap();
        let near = c.addr(g, -4).unwrap();
        let low = c.int_of_addr(t, TypeId::I32, far).unwrap();
        assert_eq!(c.get(low), Const::IntOfAddr { ty: TypeId::I32, addr: near });
        assert_eq!(c.int_of_addr(t, TypeId::I32, near).unwrap(), low);
        assert_ne!(
            c.int_of_addr(t, TypeId::I64, far).unwrap(),
            c.int_of_addr(t, TypeId::I64, near).unwrap()
        );
        assert_eq!(kind_of(c.int_of_addr(t, TypeId::PTR, a)), IrErrorKind::TypeMismatch);
        assert_eq!(kind_of(c.int_of_addr(t, TypeId::I64, absolute)), IrErrorKind::InvalidOperand);
        assert_eq!(kind_of(c.int_of_addr(t, TypeId::I64, int)), IrErrorKind::InvalidOperand);
    }

    /// §4.2: `SymDiff` is canonical in its addend, and `a - a` is a number.
    #[test]
    fn symbol_differences() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let (a, b) = (SymId::new(1), SymId::new(2));
        let d = c.sym_diff(t, TypeId::I32, a, b, -4, false).unwrap();
        assert_eq!(c.get(d), Const::SymDiff { ty: TypeId::I32, a, b, addend: -4, plt: false });
        assert_eq!(c.ty(d), TypeId::I32);
        // The addend is reduced to the width.
        assert_eq!(c.sym_diff(t, TypeId::I32, a, b, 0xffff_fffc, false).unwrap(), d);
        assert_eq!(c.sym_diff(t, TypeId::I32, a, b, -4 + (1 << 40), false).unwrap(), d);
        assert_ne!(c.sym_diff(t, TypeId::I32, b, a, -4, false).unwrap(), d);
        assert_ne!(c.sym_diff(t, TypeId::I64, a, b, -4, false).unwrap(), d);
        assert_ne!(c.sym_diff(t, TypeId::I32, a, b, -4, true).unwrap(), d);
        assert_ne!(c.sym_diff(t, TypeId::I32, a, b, 4, false).unwrap(), d);
        assert_eq!(
            c.sym_diff(t, TypeId::I32, a, a, -4, false).unwrap(),
            c.int_signed(t, TypeId::I32, -4).unwrap()
        );
        let plt_self = c.sym_diff(t, TypeId::I32, a, a, 0, true).unwrap();
        assert_eq!(
            c.get(plt_self),
            Const::SymDiff { ty: TypeId::I32, a, b: a, addend: 0, plt: true }
        );
        assert_eq!(kind_of(c.sym_diff(t, TypeId::I128, a, b, 0, false)), IrErrorKind::TypeMismatch);
        assert_eq!(kind_of(c.sym_diff(t, TypeId::PTR, a, b, 0, false)), IrErrorKind::TypeMismatch);
    }

    /// §4.2: `Expr` keeps the operator and operands, and interns.
    #[test]
    fn expressions() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let g = c.addr(SymId::new(1), 0).unwrap();
        let idx = c.undef(t, TypeId::I64).unwrap();
        let e = c.expr(ConstOp::GetElementPtr, TypeId::PTR, Some(TypeId::I32), &[g, idx]).unwrap();
        assert_eq!(
            c.get(e),
            Const::Expr {
                op: ConstOp::GetElementPtr,
                ty: TypeId::PTR,
                source_ty: Some(TypeId::I32),
                args: &[g, idx]
            }
        );
        assert_eq!(c.ty(e), TypeId::PTR);
        assert_eq!(
            c.expr(ConstOp::GetElementPtr, TypeId::PTR, Some(TypeId::I32), &[g, idx]).unwrap(),
            e
        );
        assert_ne!(
            c.expr(ConstOp::GetElementPtr, TypeId::PTR, Some(TypeId::I8), &[g, idx]).unwrap(),
            e
        );
        assert_ne!(c.expr(ConstOp::GetElementPtr, TypeId::PTR, None, &[g, idx]).unwrap(), e);
        assert_ne!(
            c.expr(ConstOp::GetElementPtr, TypeId::PTR, Some(TypeId::I32), &[g]).unwrap(),
            e
        );
        let cast = c.expr(ConstOp::PtrToInt, TypeId::I64, None, &[e]).unwrap();
        assert_ne!(c.expr(ConstOp::PtrToAddr, TypeId::I64, None, &[e]).unwrap(), cast);
        assert_eq!(
            c.get(cast),
            Const::Expr { op: ConstOp::PtrToInt, ty: TypeId::I64, source_ty: None, args: &[e] }
        );
        // Every operator has a distinct LLVM name.
        let ops = [
            ConstOp::Trunc,
            ConstOp::ZExt,
            ConstOp::SExt,
            ConstOp::PtrToInt,
            ConstOp::PtrToAddr,
            ConstOp::IntToPtr,
            ConstOp::BitCast,
            ConstOp::AddrSpaceCast,
            ConstOp::GetElementPtr,
            ConstOp::Add,
            ConstOp::Sub,
            ConstOp::Mul,
            ConstOp::Xor,
            ConstOp::And,
            ConstOp::Or,
            ConstOp::Shl,
            ConstOp::ExtractElement,
            ConstOp::InsertElement,
            ConstOp::ShuffleVector,
        ];
        let mut names: Vec<&str> = ops.iter().map(|op| op.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ops.len());
        assert_eq!(ops.iter().filter(|op| op.is_cast()).count(), 8);
        assert_eq!(ops.iter().filter(|op| op.is_binary()).count(), 7);
        assert!(!ConstOp::GetElementPtr.is_cast() && !ConstOp::GetElementPtr.is_binary());
    }

    /// §4: ids are dense and interning is stable under pool growth.
    #[test]
    fn many_constants_stay_distinct() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let ids: Vec<ConstId> = (0..3000).map(|i| c.int(t, TypeId::I32, i).unwrap()).collect();
        let wides: Vec<ConstId> = (0..300).map(|i| c.int(t, TypeId::I128, i).unwrap()).collect();
        assert_eq!(c.len(), 3300);
        assert!(!c.is_empty());
        for (i, &id) in ids.iter().enumerate() {
            assert_eq!(id.index(), i);
            assert_eq!(c.int(t, TypeId::I32, i as u64).unwrap(), id);
        }
        for (i, &id) in wides.iter().enumerate() {
            assert_eq!(c.int(t, TypeId::I128, i as u64).unwrap(), id);
        }
        assert_eq!(c.len(), 3300);
    }
}
