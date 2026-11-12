//! Flattening a constant initializer into bytes and fixups (`spec/ir.md`
//! §4.3): what a data section holds for a global variable.
//!
//! `spec/ir.md` has no §4.3 yet; its proposed text is in
//! `spec/landed/ir/WP-002.md`.
//!
//! The image of a constant of type `T` is `alloc_size_of(T)` bytes, laid
//! out as LLVM's `AsmPrinter` emits a global:
//!
//! - Padding (between struct fields, after an array element, after the
//!   value up to its alloc size) is zero. `undef` and `poison` are zero.
//!   One exception keeps the image identical to LLVM's: a vector of plain
//!   `i8`/`i16`/`i32`/`i64`/`half`/`bfloat`/`float`/`double` values whose
//!   bytes are all the same byte has its tail padding filled with that byte
//!   (LLVM emits such a `ConstantDataVector` as one fill of its alloc
//!   size), so `<3 x i8> <i8 1, i8 1, i8 1>` is `01 01 01 01`.
//! - An integer is its store size in little-endian bytes; the bits above
//!   its width are zero. `x86_fp80` is ten bytes.
//! - A vector is packed: element `i` of a vector whose elements are whole
//!   bytes starts at byte `i * bits / 8`, and elements that are not whole
//!   bytes (`<8 x i1>`, `<4 x i3>`) are packed bit by bit, element 0 in the
//!   lowest bits.
//! - An address is zero bytes in the image and a [`Fixup`] beside it.
//!
//! Only little-endian targets are supported.

use crate::constant::{Const, ConstOp, ConstPool};
use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::{BlockId, ConstId, SymId, TypeId};
use crate::types::{TypeKind, TypeTable};

/// A place in a flattened initializer whose value the linker supplies.
///
/// The bytes at `offset..offset + size` are zero in the image; the value to
/// store there is `target + addend`, truncated to `size` bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Fixup {
    /// Position in the byte buffer given to [`ConstPool::flatten`].
    pub offset: u64,
    /// Width of the field in bytes: 1, 2, 4 or 8.
    pub size: u8,
    /// What the field refers to.
    pub target: FixupTarget,
    /// The constant added to the target.
    pub addend: i64,
}

/// What a [`Fixup`] refers to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FixupTarget {
    /// The address of a symbol.
    Sym(SymId),
    /// The address of `a` minus the address of `b`.
    Diff {
        /// The symbol added.
        a: SymId,
        /// The symbol subtracted.
        b: SymId,
    },
    /// The address of `a`, or of a PLT entry for it, minus the address of
    /// `b` (`dso_local_equivalent @a` in a difference).
    PltDiff {
        /// The function.
        a: SymId,
        /// The symbol subtracted.
        b: SymId,
    },
    /// The address of the function, or of a PLT entry for it
    /// (`dso_local_equivalent` on its own).
    Plt(SymId),
    /// The address of a basic block of a function (`blockaddress`).
    Block {
        /// The function.
        func: SymId,
        /// The block.
        block: BlockId,
    },
    /// The address of block `a` minus the address of block `b`, both of
    /// function `func` (a label difference, WP-095): a number once the
    /// function is laid out, never a relocation.
    BlockDiff {
        /// The function.
        func: SymId,
        /// The block added.
        a: BlockId,
        /// The block subtracted.
        b: BlockId,
    },
}

/// A label difference: `a - b + addend` of two blocks of one function
/// (see [`ConstPool::block_diff`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BlockDiff {
    /// The function.
    pub func: SymId,
    /// The block added.
    pub a: BlockId,
    /// The block subtracted.
    pub b: BlockId,
    /// The number added.
    pub addend: i64,
}

impl ConstPool {
    /// Constant `c` as a label difference (WP-095): an integer expression
    /// built only of `ptrtoint` of a `blockaddress`, numbers, `add`, `sub`
    /// and `trunc`, in which exactly one block address is added and one
    /// subtracted, both of the same function. Clang writes
    /// `trunc (sub (ptrtoint blockaddress(@f, %a)), (ptrtoint
    /// blockaddress(@f, %b)))` for `&&a - &&b` in a static initializer.
    ///
    /// Arithmetic modulo 2^n commutes with truncation, so the value of `c`
    /// is the low bits of `a - b + addend` whatever the widths in between.
    /// `None` for any other constant.
    pub fn block_diff(&self, c: ConstId) -> Option<BlockDiff> {
        let mut term = Term::default();
        self.block_terms(c, false, 0, &mut term)?;
        match term {
            Term { func: Some(func), pos: Some(a), neg: Some(b), addend } => {
                Some(BlockDiff { func, a, b, addend })
            }
            _ => None,
        }
    }

    /// Adds `c` (negated when `negate`) to `term`; `None` when `c` has
    /// another form or `term` would hold more than one block each way.
    fn block_terms(&self, c: ConstId, negate: bool, depth: u32, term: &mut Term) -> Option<()> {
        if depth > 8 {
            return None;
        }
        match self.get(c) {
            Const::Int { value, .. } => {
                let value = value as i64;
                term.addend =
                    term.addend.wrapping_add(if negate { value.wrapping_neg() } else { value });
            }
            Const::IntOfAddr { addr, .. } => {
                let Const::BlockAddr { func, block } = self.get(addr) else { return None };
                if term.func.is_some_and(|f| f != func) {
                    return None;
                }
                term.func = Some(func);
                let side = if negate { &mut term.neg } else { &mut term.pos };
                if side.replace(block).is_some() {
                    return None;
                }
            }
            Const::Expr { op: ConstOp::Trunc, args, .. } => {
                self.block_terms(*args.first()?, negate, depth + 1, term)?;
            }
            Const::Expr { op: op @ (ConstOp::Add | ConstOp::Sub), args, .. } => {
                let [lhs, rhs] = args[..] else { return None };
                self.block_terms(lhs, negate, depth + 1, term)?;
                self.block_terms(rhs, negate != (op == ConstOp::Sub), depth + 1, term)?;
            }
            _ => return None,
        }
        Some(())
    }

    /// Appends the image of constant `c` to `bytes`, and a [`Fixup`] to
    /// `fixups` for each address in it.
    ///
    /// Exactly `types.alloc_size_of(self.ty(c))` bytes are appended. Fixup
    /// offsets are positions in `bytes`, so a caller laying several
    /// variables into one section buffer gets section offsets directly.
    /// Fixups are appended in increasing offset order.
    ///
    /// A zero constant ([`ConstPool::is_zero`]) appends that many zero
    /// bytes; a caller placing such a variable in `.bss` does not call
    /// this.
    ///
    /// Errors, all leaving `bytes` and `fixups` as they were:
    /// - `UnsizedType` when the type has no size, and `InvalidType` when it
    ///   is scalable;
    /// - `UnsupportedTarget` on a big-endian data layout;
    /// - `TypeTooLarge` when the image does not fit the address space of
    ///   the compiler;
    /// - `UnsupportedInitializer` for a constant with no image: an `Expr`,
    ///   or an `IntOfAddr` or `SymDiff` whose width is not 8, 16, 32 or 64
    ///   bits, or one inside a vector of elements that are not whole bytes.
    pub fn flatten(
        &self,
        types: &TypeTable,
        c: ConstId,
        bytes: &mut Vec<u8>,
        fixups: &mut Vec<Fixup>,
    ) -> IrResult<()> {
        let ty = self.ty(c);
        if !types.is_sized(ty) {
            return err(IrErrorKind::UnsizedType, "an initializer needs a sized type");
        }
        if types.is_scalable(ty) {
            return err(IrErrorKind::InvalidType, "an initializer cannot be scalable");
        }
        if types.data_layout().big_endian {
            return err(IrErrorKind::UnsupportedTarget, "big-endian initializers");
        }
        let (start, fixup_start) = (bytes.len(), fixups.len());
        let end = usize::try_from(types.alloc_size_of(ty))
            .ok()
            .and_then(|size| start.checked_add(size))
            .filter(|&end| isize::try_from(end).is_ok());
        let Some(end) = end else {
            return err(IrErrorKind::TypeTooLarge, "initializer is larger than the address space");
        };
        bytes.resize(end, 0);
        let mut writer = Writer { pool: self, types, bytes, fixups };
        let result = writer.write(c, start);
        if result.is_err() {
            bytes.truncate(start);
            fixups.truncate(fixup_start);
        }
        result
    }
}

/// The blocks and number gathered by [`ConstPool::block_terms`].
#[derive(Default)]
struct Term {
    func: Option<SymId>,
    pos: Option<BlockId>,
    neg: Option<BlockId>,
    addend: i64,
}

struct Writer<'a> {
    pool: &'a ConstPool,
    types: &'a TypeTable,
    bytes: &'a mut Vec<u8>,
    fixups: &'a mut Vec<Fixup>,
}

impl Writer<'_> {
    /// Writes constant `c` at byte `at`. The buffer is already zero there.
    fn write(&mut self, c: ConstId, at: usize) -> IrResult<()> {
        let (pool, types) = (self.pool, self.types);
        match pool.get(c) {
            Const::Int { ty, value } => {
                self.put(at, &value.to_le_bytes(), types.size_of(ty));
            }
            Const::WideInt { ty, words } => {
                let size = types.size_of(ty) as usize;
                let all = words.iter().flat_map(|word| word.to_le_bytes());
                for (slot, byte) in self.bytes[at..at + size].iter_mut().zip(all) {
                    *slot = byte;
                }
            }
            Const::Float { ty, bits } => self.put(at, &bits.to_le_bytes(), types.size_of(ty)),
            Const::WideFloat { ty, bits } => self.put(at, &bits.to_le_bytes(), types.size_of(ty)),
            Const::AddrOfInt { value } => {
                self.put(at, &value.to_le_bytes(), types.size_of(TypeId::PTR));
            }
            Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => {}
            Const::Bytes { bytes, .. } => self.bytes[at..at + bytes.len()].copy_from_slice(bytes),
            Const::Aggregate { ty, elems } => match types.kind(ty) {
                TypeKind::Struct { .. } => {
                    for (index, &elem) in elems.iter().enumerate() {
                        self.write(elem, at + types.field_offset(ty, index as u32) as usize)?;
                    }
                }
                TypeKind::Array { elem: elem_ty, .. } => {
                    let stride = types.alloc_size_of(elem_ty) as usize;
                    for (index, &elem) in elems.iter().enumerate() {
                        self.write(elem, at + index * stride)?;
                    }
                }
                _ => {
                    for (index, &elem) in elems.iter().enumerate() {
                        self.write_vector_element(ty, elem, at, index)?;
                    }
                    if elems.iter().all(|&elem| self.is_plain_data(elem)) {
                        self.fill_repeated_vector(ty, at);
                    }
                }
            },
            Const::Splat { ty, elem } => {
                let TypeKind::Vector { len, .. } = types.kind(ty) else {
                    unreachable!("scalable initializers were rejected");
                };
                for index in 0..len as usize {
                    self.write_vector_element(ty, elem, at, index)?;
                }
                if self.is_plain_data(elem) {
                    self.fill_repeated_vector(ty, at);
                }
            }
            Const::Addr { .. }
            | Const::BlockAddr { .. }
            | Const::DsoLocalEquivalent(_)
            | Const::NoCfi(_) => {
                let size = types.size_of(TypeId::PTR) as u8;
                self.address(c, at, size)?;
            }
            Const::IntOfAddr { ty, addr } => {
                let size = self.relocatable_size(ty, "ptrtoint")?;
                self.address(addr, at, size)?;
            }
            Const::SymDiff { ty, a, b, addend, plt } => {
                let size = self.relocatable_size(ty, "symbol difference")?;
                let target =
                    if plt { FixupTarget::PltDiff { a, b } } else { FixupTarget::Diff { a, b } };
                self.fixups.push(Fixup { offset: at as u64, size, target, addend });
            }
            Const::NoneToken => {
                return err(IrErrorKind::UnsupportedInitializer, "a token has no image");
            }
            Const::Expr { ty, op, .. } => {
                let Some(BlockDiff { func, a, b, addend }) = pool.block_diff(c) else {
                    return err(
                        IrErrorKind::UnsupportedInitializer,
                        format!("constant expression '{}' in an initializer", op.name()),
                    );
                };
                let size = self.relocatable_size(ty, "label difference")?;
                let target = FixupTarget::BlockDiff { func, a, b };
                self.fixups.push(Fixup { offset: at as u64, size, target, addend });
            }
        }
        Ok(())
    }

    /// Copies the low `size` bytes of `value` to `at`.
    fn put(&mut self, at: usize, value: &[u8], size: u64) {
        let size = size as usize;
        self.bytes[at..at + size].copy_from_slice(&value[..size]);
    }

    /// Records the fixup for address constant `addr` in a field of `size`
    /// bytes at `at`.
    fn address(&mut self, addr: ConstId, at: usize, size: u8) -> IrResult<()> {
        let (target, addend) = match self.pool.get(addr) {
            Const::Addr { sym, addend } => (FixupTarget::Sym(sym), addend),
            Const::NoCfi(sym) => (FixupTarget::Sym(sym), 0),
            Const::DsoLocalEquivalent(sym) => (FixupTarget::Plt(sym), 0),
            Const::BlockAddr { func, block } => (FixupTarget::Block { func, block }, 0),
            _ => return err(IrErrorKind::UnsupportedInitializer, "not an address constant"),
        };
        self.fixups.push(Fixup { offset: at as u64, size, target, addend });
        Ok(())
    }

    /// The byte size of an integer field that can hold a relocation.
    fn relocatable_size(&self, ty: TypeId, what: &str) -> IrResult<u8> {
        match self.types.int_bits(ty) {
            Some(bits @ (8 | 16 | 32 | 64)) => Ok((bits / 8) as u8),
            _ => err(
                IrErrorKind::UnsupportedInitializer,
                format!("{what} of a width no relocation has"),
            ),
        }
    }

    /// Whether `elem` is an integer or floating-point number (not `undef`,
    /// not an address) of a type LLVM stores as packed data: `i8`, `i16`,
    /// `i32`, `i64`, `half`, `bfloat`, `float` or `double`.
    fn is_plain_data(&self, elem: ConstId) -> bool {
        match self.pool.get(elem) {
            Const::Int { ty, .. } => matches!(self.types.int_bits(ty), Some(8 | 16 | 32 | 64)),
            Const::Float { .. } => true,
            _ => false,
        }
    }

    /// Fills the tail padding of the vector of plain data of type `ty`
    /// written at `at` with the vector's byte, when all its bytes are the
    /// same one. LLVM emits such a vector as a single fill of its alloc
    /// size; the padding is not part of the value either way.
    fn fill_repeated_vector(&mut self, ty: TypeId, at: usize) {
        let store = self.types.size_of(ty) as usize;
        let alloc = self.types.alloc_size_of(ty) as usize;
        let byte = self.bytes[at];
        if self.bytes[at..at + store].iter().all(|&other| other == byte) {
            self.bytes[at + store..at + alloc].fill(byte);
        }
    }

    /// Writes element `index` of a constant of vector type `ty` that
    /// starts at byte `at`.
    fn write_vector_element(
        &mut self,
        ty: TypeId,
        elem: ConstId,
        at: usize,
        index: usize,
    ) -> IrResult<()> {
        let types = self.types;
        let elem_ty = types.element_type(ty).unwrap_or(ty);
        let elem_bits = types.size_in_bits_of(elem_ty) as usize;
        if elem_bits.is_multiple_of(8) {
            return self.write(elem, at + index * (elem_bits / 8));
        }
        // Elements that are not whole bytes: the vector is one integer with
        // element 0 in its lowest bits.
        let value = match self.pool.get(elem) {
            Const::Int { value, .. } => vec![value],
            Const::WideInt { words, .. } => words.to_vec(),
            Const::Undef(_) | Const::Poison(_) => return Ok(()),
            _ => {
                return err(
                    IrErrorKind::UnsupportedInitializer,
                    "a relocatable element in a vector of elements that are not whole bytes",
                );
            }
        };
        let first = index * elem_bits;
        for bit in 0..elem_bits {
            if (value[bit / 64] >> (bit % 64)) & 1 == 1 {
                let position = first + bit;
                self.bytes[at + position / 8] |= 1 << (position % 8);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
// The tests name the two tables `t` and `c` and symbols `f` and `g`.
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;
    use crate::constant::ConstOp;
    use crate::module::Module;
    use crate::target::TargetSpec;

    fn module() -> Module {
        Module::new(TargetSpec::x86_64_linux_gnu())
    }

    fn image(m: &Module, c: ConstId) -> (Vec<u8>, Vec<Fixup>) {
        let (mut bytes, mut fixups) = (Vec::new(), Vec::new());
        m.consts.flatten(&m.types, c, &mut bytes, &mut fixups).unwrap();
        assert_eq!(bytes.len() as u64, m.types.alloc_size_of(m.consts.ty(c)));
        (bytes, fixups)
    }

    fn fails(m: &Module, c: ConstId) -> IrErrorKind {
        let (mut bytes, mut fixups) = (vec![0xaa; 3], Vec::new());
        let error = m.consts.flatten(&m.types, c, &mut bytes, &mut fixups).unwrap_err();
        // A failed flatten leaves the buffers as they were.
        assert_eq!(bytes, [0xaa; 3]);
        assert!(fixups.is_empty());
        error.kind
    }

    /// §4.3: scalars are little-endian at their store size, zero-padded to
    /// the alloc size.
    #[test]
    fn scalars() {
        let mut m = module();
        let c = m.consts.int(&m.types, TypeId::I32, 0x1234_5678).unwrap();
        assert_eq!(image(&m, c).0, [0x78, 0x56, 0x34, 0x12]);
        let i3 = m.types.int(3).unwrap();
        let c = m.consts.int_signed(&m.types, i3, -1).unwrap();
        assert_eq!(image(&m, c).0, [7]);
        let i24 = m.types.int(24).unwrap();
        let c = m.consts.int_signed(&m.types, i24, -2).unwrap();
        assert_eq!(image(&m, c).0, [0xfe, 0xff, 0xff, 0]);
        let i65 = m.types.int(65).unwrap();
        let c = m.consts.int_signed(&m.types, i65, -2).unwrap();
        let mut expected = vec![0xff; 8];
        expected[0] = 0xfe;
        expected.extend([1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(image(&m, c).0, expected);
        let c = m.consts.f32(1.0).unwrap();
        assert_eq!(image(&m, c).0, 1.0f32.to_le_bytes());
        let c = m.consts.f64(-2.5).unwrap();
        assert_eq!(image(&m, c).0, (-2.5f64).to_le_bytes());
        let c = m.consts.float(&m.types, TypeId::HALF, 0x3c00).unwrap();
        assert_eq!(image(&m, c).0, [0x00, 0x3c]);
        let c = m.consts.float(&m.types, TypeId::X86_FP80, 0x3fff_8000_0000_0000_0000).unwrap();
        assert_eq!(image(&m, c).0, [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f, 0, 0, 0, 0, 0, 0]);
        let c = m.consts.float(&m.types, TypeId::FP128, 0x3fff << 112).unwrap();
        let mut expected = vec![0; 14];
        expected.extend([0xff, 0x3f]);
        assert_eq!(image(&m, c).0, expected);
        let c = m.consts.addr_of_int(0x1122).unwrap();
        assert_eq!(image(&m, c), (vec![0x22, 0x11, 0, 0, 0, 0, 0, 0], vec![]));
        for c in [
            m.consts.null(&m.types, TypeId::PTR).unwrap(),
            m.consts.undef(&m.types, TypeId::I64).unwrap(),
            m.consts.poison(&m.types, TypeId::DOUBLE).unwrap(),
        ] {
            assert_eq!(image(&m, c), (vec![0; 8], vec![]));
        }
    }

    /// §4.3: aggregates place each element at its offset; padding is zero.
    #[test]
    fn aggregates_and_padding() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let inner = t.struct_type(&[TypeId::I8, TypeId::I16], false).unwrap();
        let outer = t.struct_type(&[TypeId::I8, inner, TypeId::I64, TypeId::I8], false).unwrap();
        let b = |c: &mut ConstPool, t: &TypeTable, v| c.int(t, TypeId::I8, v).unwrap();
        let (b1, b2, b3) = (b(c, t, 0x11), b(c, t, 0x22), b(c, t, 0x33));
        let short = c.int(t, TypeId::I16, 0xbbaa).unwrap();
        let long = c.int(t, TypeId::I64, 0x0102_0304_0506_0708).unwrap();
        let inner_c = c.aggregate(t, inner, &[b2, short]).unwrap();
        let outer_c = c.aggregate(t, outer, &[b1, inner_c, long, b3]).unwrap();
        let arr = t.array(outer, 2).unwrap();
        let zero = c.zero(t, outer).unwrap();
        let arr_c = c.aggregate(t, arr, &[zero, outer_c]).unwrap();
        let packed = t.struct_type(&[TypeId::I8, TypeId::I32], true).unwrap();
        let int = c.int(t, TypeId::I32, 0xdead_beef).unwrap();
        let packed_c = c.aggregate(t, packed, &[b1, int]).unwrap();
        let text = c.bytes(t, b"hod\0").unwrap();
        let one_expected: Vec<u8> = [
            &[0x11, 0][..],
            &[0x22, 0, 0xaa, 0xbb],
            &[0, 0],
            &[8, 7, 6, 5, 4, 3, 2, 1],
            &[0x33, 0, 0, 0, 0, 0, 0, 0],
        ]
        .concat();
        assert_eq!(image(&m, outer_c).0, one_expected);
        assert_eq!(image(&m, arr_c).0, [vec![0; 24], one_expected].concat());
        assert_eq!(image(&m, packed_c).0, [0x11, 0xef, 0xbe, 0xad, 0xde]);
        assert_eq!(image(&m, text).0, b"hod\0");
        assert_eq!(image(&m, zero), (vec![0; 24], vec![]));
    }

    /// §4.3: vectors are packed, bit by bit when the element is not whole
    /// bytes.
    #[test]
    fn vectors() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let bits = t.vector(TypeId::I1, 5).unwrap();
        let (yes, no) = (c.int(t, TypeId::I1, 1).unwrap(), c.int(t, TypeId::I1, 0).unwrap());
        let mask = c.aggregate(t, bits, &[yes, no, yes, yes, no]).unwrap();
        let i3 = t.int(3).unwrap();
        let v3 = t.vector(i3, 4).unwrap();
        let threes: Vec<ConstId> = [1, 2, 3, 7].iter().map(|&v| c.int(t, i3, v).unwrap()).collect();
        let packed = c.aggregate(t, v3, &threes).unwrap();
        let i24 = t.int(24).unwrap();
        let v24 = t.vector(i24, 3).unwrap();
        let wide = c.int(t, i24, 0x00c0_ffee).unwrap();
        let splat = c.splat(t, v24, wide).unwrap();
        let vf = t.vector(TypeId::FLOAT, 2).unwrap();
        let (one, two) = (c.f32(1.0).unwrap(), c.f32(2.0).unwrap());
        let floats = c.aggregate(t, vf, &[one, two]).unwrap();
        let vx = t.vector(TypeId::X86_FP80, 2).unwrap();
        let x87 = c.float(t, TypeId::X86_FP80, 0x3fff_8000_0000_0000_0000).unwrap();
        let x87s = c.splat(t, vx, x87).unwrap();
        let undef1 = c.undef(t, TypeId::I1).unwrap();
        let partial = c.aggregate(t, bits, &[yes, undef1, undef1, yes, yes]).unwrap();
        let vp = t.vector(TypeId::PTR, 2).unwrap();
        let (p, q) = (c.addr(SymId::new(4), 1).unwrap(), c.null(t, TypeId::PTR).unwrap());
        let pointers = c.aggregate(t, vp, &[q, p]).unwrap();

        assert_eq!(image(&m, mask).0, [0b0_1101]);
        // 1 | 2 << 3 | 3 << 6 | 7 << 9 = 0xed1
        assert_eq!(image(&m, packed).0, [0xd1, 0x0e]);
        let mut expected = [0xee, 0xff, 0xc0].repeat(3);
        expected.resize(16, 0);
        assert_eq!(image(&m, splat).0, expected);
        assert_eq!(image(&m, floats).0, [1.0f32.to_le_bytes(), 2.0f32.to_le_bytes()].concat());
        let one_x87 = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
        let mut expected = [one_x87, one_x87].concat();
        expected.resize(32, 0);
        assert_eq!(image(&m, x87s).0, expected);
        assert_eq!(image(&m, partial).0, [0b1_1001]);
        let fixup =
            Fixup { offset: 8, size: 8, target: FixupTarget::Sym(SymId::new(4)), addend: 1 };
        assert_eq!(image(&m, pointers), (vec![0; 16], vec![fixup]));
    }

    /// §4.3: the tail padding of a vector of plain data whose bytes are all
    /// one byte repeats that byte, as LLVM's fill does; all other padding
    /// is zero.
    #[test]
    fn repeated_byte_vector_padding() {
        let mut m = module();
        let v3 = m.types.vector(TypeId::I8, 3).unwrap();
        let seven = m.consts.int(&m.types, TypeId::I8, 7).unwrap();
        let eight = m.consts.int(&m.types, TypeId::I8, 8).unwrap();
        let undef = m.consts.undef(&m.types, TypeId::I8).unwrap();
        let c = m.consts.splat(&m.types, v3, seven).unwrap();
        assert_eq!(image(&m, c).0, [7, 7, 7, 7]);
        let c = m.consts.aggregate(&m.types, v3, &[seven, seven, eight]).unwrap();
        assert_eq!(image(&m, c).0, [7, 7, 8, 0]);
        let c = m.consts.aggregate(&m.types, v3, &[seven, undef, seven]).unwrap();
        assert_eq!(image(&m, c).0, [7, 0, 7, 0]);

        let halves = m.types.vector(TypeId::HALF, 3).unwrap();
        let half = m.consts.float(&m.types, TypeId::HALF, 0x4040).unwrap();
        let c = m.consts.splat(&m.types, halves, half).unwrap();
        assert_eq!(image(&m, c).0, [0x40; 8]);
        let half = m.consts.float(&m.types, TypeId::HALF, 0x4041).unwrap();
        let c = m.consts.splat(&m.types, halves, half).unwrap();
        assert_eq!(image(&m, c).0, [0x41, 0x40, 0x41, 0x40, 0x41, 0x40, 0, 0]);

        // `i24` is not a type LLVM stores as packed data.
        let i24 = m.types.int(24).unwrap();
        let v3 = m.types.vector(i24, 3).unwrap();
        let ones = m.consts.int(&m.types, i24, 0x01_0101).unwrap();
        let c = m.consts.splat(&m.types, v3, ones).unwrap();
        let mut expected = vec![1; 9];
        expected.resize(16, 0);
        assert_eq!(image(&m, c).0, expected);
    }

    /// §4.3: addresses become fixups at their offsets in the buffer.
    #[test]
    fn fixups() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let (f, g) = (SymId::new(1), SymId::new(2));
        let addr = c.addr(g, 24).unwrap();
        let int32 = c.fold_cast(t, ConstOp::PtrToInt, addr, TypeId::I32).unwrap();
        let diff = c.sym_diff(t, TypeId::I32, f, g, -4, false).unwrap();
        let plt_diff = c.sym_diff(t, TypeId::I32, f, g, 0, true).unwrap();
        let block = c.block_addr(f, BlockId::new(2)).unwrap();
        let dso = c.dso_local_equivalent(f).unwrap();
        let no_cfi = c.no_cfi(f).unwrap();
        let byte = c.int(t, TypeId::I8, 9).unwrap();
        let st = t
            .struct_type(
                &[TypeId::I8, TypeId::PTR, TypeId::I32, TypeId::I32, TypeId::I32, TypeId::PTR],
                false,
            )
            .unwrap();
        let fields = [byte, addr, int32, diff, plt_diff, block];
        let record = c.aggregate(t, st, &fields).unwrap();
        let pair = t.array(TypeId::PTR, 2).unwrap();
        let others = c.aggregate(t, pair, &[dso, no_cfi]).unwrap();

        let fixup = |offset, size, target, addend| Fixup { offset, size, target, addend };
        let (bytes, fixups) = image(&m, record);
        let mut expected = vec![0; 40];
        expected[0] = 9;
        assert_eq!(bytes, expected);
        assert_eq!(
            fixups,
            [
                fixup(8, 8, FixupTarget::Sym(g), 24),
                fixup(16, 4, FixupTarget::Sym(g), 24),
                fixup(20, 4, FixupTarget::Diff { a: f, b: g }, -4),
                fixup(24, 4, FixupTarget::PltDiff { a: f, b: g }, 0),
                fixup(32, 8, FixupTarget::Block { func: f, block: BlockId::new(2) }, 0),
            ]
        );
        // Offsets are positions in the caller's buffer.
        let (mut bytes, mut fixups) = (vec![0xcc; 5], vec![fixup(0, 1, FixupTarget::Sym(f), 0)]);
        m.consts.flatten(&m.types, others, &mut bytes, &mut fixups).unwrap();
        assert_eq!(bytes.len(), 21);
        assert_eq!(bytes[..5], [0xcc; 5]);
        assert_eq!(
            fixups[1..],
            [fixup(5, 8, FixupTarget::Plt(f), 0), fixup(13, 8, FixupTarget::Sym(f), 0)]
        );
        // 40 since WP-095: `FixupTarget::BlockDiff` holds three ids.
        assert_eq!(size_of::<Fixup>(), 40);
    }

    /// §4.3 errors: `UnsupportedInitializer`, `UnsizedType`, `InvalidType`,
    /// `UnsupportedTarget`.
    #[test]
    fn constants_with_no_image() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let (f, g) = (SymId::new(1), SymId::new(2));
        let addr = c.addr(g, 0).unwrap();
        let unsupported = IrErrorKind::UnsupportedInitializer;
        // An expression, also when nested.
        let int = c.fold_cast(t, ConstOp::PtrToInt, addr, TypeId::I64).unwrap();
        let two = c.int(t, TypeId::I64, 2).unwrap();
        let product = c.fold_binary(t, ConstOp::Mul, int, two).unwrap();
        let arr = t.array(TypeId::I64, 2).unwrap();
        let nested = c.aggregate(t, arr, &[two, product]).unwrap();
        // Widths with no relocation.
        let wide = c.fold_cast(t, ConstOp::PtrToInt, addr, TypeId::I128).unwrap();
        let i24 = t.int(24).unwrap();
        let odd = c.fold_cast(t, ConstOp::PtrToInt, addr, i24).unwrap();
        let odd_diff = c.sym_diff(t, i24, f, g, 0, false).unwrap();
        let token = c.none_token().unwrap();
        let scalable = t.scalable_vector(TypeId::I32, 4).unwrap();
        let scalable_zero = c.zero(t, scalable).unwrap();
        let ext = t.target_ext("x").unwrap();
        let ext_zero = c.zero(t, ext).unwrap();
        // Good neighbours.
        let byte_diff = c.sym_diff(t, TypeId::I8, f, g, 0, false).unwrap();

        let error = {
            let (mut bytes, mut fixups) = (Vec::new(), Vec::new());
            m.consts.flatten(&m.types, product, &mut bytes, &mut fixups).unwrap_err()
        };
        assert!(error.message.contains("'mul'"), "{error}");
        for bad in [product, nested, wide, odd, odd_diff] {
            assert_eq!(fails(&m, bad), unsupported);
        }
        assert_eq!(fails(&m, token), IrErrorKind::UnsizedType);
        assert_eq!(fails(&m, ext_zero), IrErrorKind::UnsizedType);
        assert_eq!(fails(&m, scalable_zero), IrErrorKind::InvalidType);
        let diff_fixup =
            Fixup { offset: 0, size: 1, target: FixupTarget::Diff { a: f, b: g }, addend: 0 };
        assert_eq!(image(&m, byte_diff), (vec![0], vec![diff_fixup]));

        let big_endian = crate::target::DataLayout::parse("E-i64:64").unwrap();
        let types = TypeTable::new(&big_endian);
        let mut pool = ConstPool::new();
        let one = pool.int(&types, TypeId::I32, 1).unwrap();
        let (mut bytes, mut fixups) = (Vec::new(), Vec::new());
        let error = pool.flatten(&types, one, &mut bytes, &mut fixups).unwrap_err();
        assert_eq!(error.kind, IrErrorKind::UnsupportedTarget);
    }
}
