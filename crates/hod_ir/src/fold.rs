//! Folding LLVM constant expressions into normal forms (`spec/ir.md` §4.2).
//!
//! A producer that meets a constant expression calls the matching function
//! here with the already-folded operands. The result is a normal form when
//! one exists and a [`Const::Expr`] otherwise; an `Expr` is not an error
//! here, it is what `classify` later reports by operator name. Errors are
//! reserved for expressions LLVM itself would reject: operands of the wrong
//! type, a struct index out of range, an unsized source type.
//!
//! The folds are exact for the two supported targets: pointers of address
//! space 0 are 64 bits wide and an address is `symbol + addend` modulo
//! `2^64`.

use crate::constant::{Const, ConstOp, ConstPool};
use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::{ConstId, SymId, TypeId};
use crate::types::{TypeKind, TypeTable};
use crate::wide;

/// `pos - neg + addend`, modulo two to the width of the operand's type.
#[derive(Clone, Copy)]
struct Linear {
    /// The symbol added, and whether it is `dso_local_equivalent`.
    pos: Option<(SymId, bool)>,
    /// The symbol subtracted.
    neg: Option<SymId>,
    addend: i64,
}

impl ConstPool {
    /// Folds `getelementptr (source_ty, base, indices...)`.
    ///
    /// With constant integer indices the offset is computed from the type
    /// table, wrapping modulo `2^64` like the instruction, and applied to
    /// the base: `Addr` gains the offset, `null` and absolute addresses
    /// become the absolute address, and any base is returned unchanged for
    /// offset zero. Everything else (a non-constant-integer index, a vector
    /// of pointers, a scalable source type, a vector whose elements are not
    /// whole bytes, a base that is not `symbol + addend`) is an `Expr`.
    ///
    /// Errors: `TypeMismatch` when `base` is not a pointer or vector of
    /// pointers or an index is not an integer or vector of integers;
    /// `UnsizedType` when `source_ty` has no size; `BadIndex` when a struct
    /// is indexed by anything but an in-range integer constant, or a scalar
    /// is indexed into. `types` is mutable only to intern the vector-of-
    /// pointers result type of a vector `getelementptr`.
    pub fn fold_gep(
        &mut self,
        types: &mut TypeTable,
        source_ty: TypeId,
        base: ConstId,
        indices: &[ConstId],
    ) -> IrResult<ConstId> {
        let base_ty = self.ty(base);
        let (base_scalar, base_shape) = shape(types, base_ty);
        if !types.is_ptr(base_scalar) {
            return err(IrErrorKind::TypeMismatch, "getelementptr base is not a pointer");
        }
        let mut result_ty = base_ty;
        let mut vector_shape = base_shape;
        for (at, &index) in indices.iter().enumerate() {
            let (scalar, index_shape) = shape(types, self.ty(index));
            if !types.is_int(scalar) {
                return err(
                    IrErrorKind::TypeMismatch,
                    format!("getelementptr index {at} is not an integer"),
                );
            }
            if let Some((len, scalable)) = index_shape {
                if vector_shape.is_some_and(|s| s != (len, scalable)) {
                    return err(
                        IrErrorKind::TypeMismatch,
                        "getelementptr vector operands differ in length",
                    );
                }
                if vector_shape.is_none() {
                    vector_shape = index_shape;
                    result_ty = if scalable {
                        types.scalable_vector(base_ty, len)?
                    } else {
                        types.vector(base_ty, len)?
                    };
                }
            }
        }
        if !types.is_sized(source_ty) {
            return err(IrErrorKind::UnsizedType, "getelementptr source type has no size");
        }
        let unfolded = |pool: &mut ConstPool| {
            let mut args = Vec::with_capacity(indices.len() + 1);
            args.push(base);
            args.extend_from_slice(indices);
            pool.expr(ConstOp::GetElementPtr, result_ty, Some(source_ty), &args)
        };

        let offset = self.gep_offset(types, source_ty, indices)?;
        if types.is_scalable(source_ty) || vector_shape.is_some() {
            return unfolded(self);
        }
        let Some(offset) = offset else {
            return unfolded(self);
        };
        if offset == 0 {
            return Ok(base);
        }
        match self.get(base) {
            Const::Addr { sym, addend } => self.addr(sym, addend.wrapping_add(offset)),
            Const::Null(TypeId::PTR) => self.addr_of_int(offset as u64),
            Const::AddrOfInt { value } => self.addr_of_int(value.wrapping_add(offset as u64)),
            Const::Poison(_) => self.poison(types, result_ty),
            Const::Undef(_) => self.undef(types, result_ty),
            _ => unfolded(self),
        }
    }

    /// The byte offset a `getelementptr` over `source_ty` adds, or `None`
    /// when it is not a constant known here.
    fn gep_offset(
        &self,
        types: &TypeTable,
        source_ty: TypeId,
        indices: &[ConstId],
    ) -> IrResult<Option<i64>> {
        let mut offset = Some(0i64);
        let mut current = source_ty;
        for (at, &index) in indices.iter().enumerate() {
            let value = self.index_value(types, index);
            let stride = if at == 0 {
                (!types.is_scalable(current)).then(|| types.alloc_size_of(current))
            } else {
                match types.kind(current) {
                    TypeKind::Struct { fields, .. } => {
                        // A struct index is an unsigned constant.
                        let field = match self.get(index) {
                            Const::Int { value, .. } if value < fields.len() as u64 => value,
                            _ => {
                                return err(
                                    IrErrorKind::BadIndex,
                                    format!(
                                        "getelementptr index {at} is not a field of the struct"
                                    ),
                                );
                            }
                        };
                        let field_offset = types.field_offset(current, field as u32);
                        offset = offset.map(|o| o.wrapping_add(field_offset as i64));
                        current = fields[field as usize];
                        continue;
                    }
                    TypeKind::Array { elem, .. } => {
                        current = elem;
                        (!types.is_scalable(elem)).then(|| types.alloc_size_of(elem))
                    }
                    TypeKind::Vector { elem, .. } => {
                        let stride = types.element_stride(current);
                        current = elem;
                        stride
                    }
                    TypeKind::ScalableVector { elem, .. } => {
                        current = elem;
                        None
                    }
                    _ => {
                        return err(
                            IrErrorKind::BadIndex,
                            format!("getelementptr index {at} indexes into a scalar"),
                        );
                    }
                }
            };
            offset = match (offset, value, stride) {
                (Some(o), Some(v), Some(s)) => Some(o.wrapping_add(v.wrapping_mul(s as i64))),
                _ => None,
            };
        }
        Ok(offset)
    }

    /// The value of a `getelementptr` index: an integer constant,
    /// sign-extended or truncated to 64 bits.
    fn index_value(&self, types: &TypeTable, index: ConstId) -> Option<i64> {
        match self.get(index) {
            Const::Int { ty, value } => Some(wide::sext64(value, types.int_bits(ty).unwrap_or(64))),
            Const::WideInt { words, .. } => Some(words[0] as i64),
            _ => None,
        }
    }

    /// Folds the cast `op (value to to)`.
    ///
    /// - `trunc`, `zext`, `sext` of an integer are computed; `trunc` also
    ///   narrows `IntOfAddr` and `SymDiff`, and `zext` widens an `IntOfAddr`
    ///   that holds the whole address.
    /// - `ptrtoint` and `ptrtoaddr` give zero for `null`, the number for an
    ///   absolute address, and `IntOfAddr` for an address constant.
    /// - `inttoptr` gives the absolute address of an integer (`null` for
    ///   zero) and undoes a `ptrtoint` that kept all 64 bits.
    /// - `bitcast` is the identity on equal types and reinterprets the bits
    ///   between integers and floats of one width.
    /// - `poison` folds to `poison`, and `undef` to `undef` (to zero for
    ///   `zext` and `sext`, whose high bits are not arbitrary).
    /// - Anything else, including every cast between vectors of a non-zero
    ///   value, is an `Expr`.
    ///
    /// Errors: `InvalidOperand` when `op` is not a cast; `TypeMismatch`
    /// when LLVM would reject the cast between these two types.
    pub fn fold_cast(
        &mut self,
        types: &TypeTable,
        op: ConstOp,
        value: ConstId,
        to: TypeId,
    ) -> IrResult<ConstId> {
        if !op.is_cast() {
            return err(IrErrorKind::InvalidOperand, format!("{} is not a cast", op.name()));
        }
        let from = self.ty(value);
        check_cast(types, op, from, to)?;
        if matches!(self.get(value), Const::Poison(_)) {
            return self.poison(types, to);
        }
        if matches!(op, ConstOp::BitCast | ConstOp::AddrSpaceCast) && from == to {
            return Ok(value);
        }
        if shape(types, from).1.is_some() || shape(types, to).1.is_some() {
            if op == ConstOp::BitCast && self.is_zero(value) {
                return self.zero(types, to);
            }
            return self.expr(op, to, None, &[value]);
        }
        let pointer_bits = types.size_in_bits_of(TypeId::PTR) as u32;
        let from_bits = types.size_in_bits_of(from) as u32;
        let to_bits = types.size_in_bits_of(to) as u32;
        match (op, self.get(value)) {
            (ConstOp::Trunc | ConstOp::ZExt, Const::Int { .. } | Const::WideInt { .. }) => {
                let words = self.int_words(value).unwrap_or_default();
                self.int_from_words(types, to, &words)
            }
            (ConstOp::SExt, Const::Int { .. } | Const::WideInt { .. }) => {
                let mut words = self.int_words(value).unwrap_or_default();
                wide::sign_extend(&mut words, from_bits, to_bits);
                self.int_from_words(types, to, &words)
            }
            (ConstOp::Trunc, Const::IntOfAddr { addr, .. }) => self.int_of_addr(types, to, addr),
            (ConstOp::ZExt, Const::IntOfAddr { addr, .. }) if from_bits >= pointer_bits => {
                self.int_of_addr(types, to, addr)
            }
            (ConstOp::Trunc, Const::SymDiff { a, b, addend, plt, .. }) => {
                self.sym_diff(types, to, a, b, addend, plt)
            }
            (ConstOp::ZExt | ConstOp::SExt, Const::Undef(_)) => self.zero(types, to),
            (_, Const::Undef(_)) => self.undef(types, to),
            (ConstOp::PtrToInt | ConstOp::PtrToAddr, Const::Null(_)) => self.int(types, to, 0),
            (ConstOp::PtrToInt | ConstOp::PtrToAddr, Const::AddrOfInt { value }) => {
                self.int(types, to, value)
            }
            (ConstOp::PtrToInt | ConstOp::PtrToAddr, _) if self.is_symbolic_address(value) => {
                self.int_of_addr(types, to, value)
            }
            (ConstOp::IntToPtr, _) if self.is_zero(value) => self.null(types, to),
            (ConstOp::IntToPtr, _) if to != TypeId::PTR => self.expr(op, to, None, &[value]),
            (ConstOp::IntToPtr, Const::Int { value, .. }) => self.addr_of_int(value),
            (ConstOp::IntToPtr, Const::WideInt { words, .. }) => {
                let low = words[0];
                self.addr_of_int(low)
            }
            (ConstOp::IntToPtr, Const::IntOfAddr { addr, .. }) if from_bits >= pointer_bits => {
                Ok(addr)
            }
            (ConstOp::BitCast, Const::Int { value: bits, .. } | Const::Float { bits, .. }) => {
                self.bits_as(types, to, u128::from(bits))
            }
            (ConstOp::BitCast, Const::WideFloat { bits, .. }) => self.bits_as(types, to, bits),
            (ConstOp::BitCast, Const::WideInt { words, .. }) => {
                let bits = u128::from(words[0]) | (u128::from(words[1]) << 64);
                self.bits_as(types, to, bits)
            }
            _ => self.expr(op, to, None, &[value]),
        }
    }

    /// The integer or float of type `to` with this bit pattern.
    fn bits_as(&mut self, types: &TypeTable, to: TypeId, bits: u128) -> IrResult<ConstId> {
        if types.is_float(to) {
            self.float(types, to, bits)
        } else {
            self.int_from_words(types, to, &[bits as u64, (bits >> 64) as u64])
        }
    }

    /// Folds the binary operator `op (lhs, rhs)`.
    ///
    /// - Two integers of any width are computed for every operator (`shl`
    ///   by the width or more is `poison`).
    /// - `add` and `sub` at widths up to 64 combine the address forms:
    ///   `ptrtoint` of `symbol + addend`, `SymDiff`, and integers. The
    ///   result is an `IntOfAddr` with a new addend, a `SymDiff`
    ///   (`ptrtoint a - ptrtoint b`, plus or minus constants), or an integer
    ///   when the symbols cancel.
    /// - `poison` in either operand is `poison`.
    /// - Anything else (two symbols added, a symbol negated, `mul` or a
    ///   bitwise operator on an address, vectors) is an `Expr`.
    ///
    /// Errors: `InvalidOperand` when `op` is not a binary operator;
    /// `TypeMismatch` unless both operands have one integer or
    /// integer-vector type.
    pub fn fold_binary(
        &mut self,
        types: &TypeTable,
        op: ConstOp,
        lhs: ConstId,
        rhs: ConstId,
    ) -> IrResult<ConstId> {
        if !op.is_binary() {
            return err(
                IrErrorKind::InvalidOperand,
                format!("{} is not a binary operator", op.name()),
            );
        }
        let ty = self.ty(lhs);
        let (scalar, vector) = shape(types, ty);
        let Some(bits) = types.int_bits(scalar) else {
            return err(IrErrorKind::TypeMismatch, format!("{} needs integer operands", op.name()));
        };
        if self.ty(rhs) != ty {
            return err(
                IrErrorKind::TypeMismatch,
                format!("{} operands differ in type", op.name()),
            );
        }
        if matches!(self.get(lhs), Const::Poison(_)) || matches!(self.get(rhs), Const::Poison(_)) {
            return self.poison(types, ty);
        }
        if vector.is_some() {
            return self.expr(op, ty, None, &[lhs, rhs]);
        }
        if let (Some(a), Some(b)) = (self.int_words(lhs), self.int_words(rhs)) {
            let words = match op {
                ConstOp::Add => wide::add(&a, &b, bits),
                ConstOp::Sub => wide::sub(&a, &b, bits),
                ConstOp::Mul => wide::mul(&a, &b, bits),
                ConstOp::Xor => wide::bitwise(&a, &b, |x, y| x ^ y),
                ConstOp::And => wide::bitwise(&a, &b, |x, y| x & y),
                ConstOp::Or => wide::bitwise(&a, &b, |x, y| x | y),
                ConstOp::Shl => {
                    if b[1..].iter().any(|&w| w != 0) || b[0] >= u64::from(bits) {
                        return self.poison(types, ty);
                    }
                    wide::shl(&a, b[0] as u32, bits)
                }
                _ => unreachable!("is_binary covers exactly these operators"),
            };
            return self.int_from_words(types, ty, &words);
        }
        if bits <= 64
            && matches!(op, ConstOp::Add | ConstOp::Sub)
            && let (Some(l), Some(r)) = (self.linear(lhs), self.linear(rhs))
            && let Some(sum) = combine(l, r, op == ConstOp::Sub)
        {
            let addend = wide::sext64(sum.addend as u64 & wide::mask(bits), bits);
            match (sum.pos, sum.neg) {
                (None, None) => return self.int(types, ty, addend as u64),
                (Some((sym, false)), None) => {
                    let addr = self.addr(sym, addend)?;
                    return self.int_of_addr(types, ty, addr);
                }
                (Some((sym, true)), None) if addend == 0 => {
                    let addr = self.dso_local_equivalent(sym)?;
                    return self.int_of_addr(types, ty, addr);
                }
                (Some((a, plt)), Some(b)) => return self.sym_diff(types, ty, a, b, addend, plt),
                _ => {}
            }
        }
        self.expr(op, ty, None, &[lhs, rhs])
    }

    /// `id` as `pos - neg + addend`, when it has that form.
    fn linear(&self, id: ConstId) -> Option<Linear> {
        match self.get(id) {
            Const::Int { value, .. } => Some(Linear { pos: None, neg: None, addend: value as i64 }),
            Const::IntOfAddr { addr, .. } => match self.get(addr) {
                Const::Addr { sym, addend } => {
                    Some(Linear { pos: Some((sym, false)), neg: None, addend })
                }
                Const::DsoLocalEquivalent(sym) => {
                    Some(Linear { pos: Some((sym, true)), neg: None, addend: 0 })
                }
                _ => None,
            },
            Const::SymDiff { a, b, addend, plt, .. } => {
                Some(Linear { pos: Some((a, plt)), neg: Some(b), addend })
            }
            _ => None,
        }
    }
}

/// `l + r` or `l - r`, when the result still has at most one symbol added
/// and one subtracted.
fn combine(l: Linear, r: Linear, subtract: bool) -> Option<Linear> {
    let r = if subtract {
        // The PLT entry of a function cannot be subtracted.
        if matches!(r.pos, Some((_, true))) {
            return None;
        }
        Linear {
            pos: r.neg.map(|sym| (sym, false)),
            neg: r.pos.map(|(sym, _)| sym),
            addend: r.addend.wrapping_neg(),
        }
    } else {
        r
    };
    let pos = match (l.pos, r.pos) {
        (Some(_), Some(_)) => return None,
        (pos, None) | (None, pos) => pos,
    };
    let neg = match (l.neg, r.neg) {
        (Some(_), Some(_)) => return None,
        (neg, None) | (None, neg) => neg,
    };
    Some(Linear { pos, neg, addend: l.addend.wrapping_add(r.addend) })
}

/// The scalar type of `ty` and, for a vector, its length and whether it is
/// scalable.
fn shape(types: &TypeTable, ty: TypeId) -> (TypeId, Option<(u32, bool)>) {
    match types.kind(ty) {
        TypeKind::Vector { elem, len } => (elem, Some((len, false))),
        TypeKind::ScalableVector { elem, min_len } => (elem, Some((min_len, true))),
        _ => (ty, None),
    }
}

/// Checks that LLVM accepts the cast `op` from `from` to `to`.
fn check_cast(types: &TypeTable, op: ConstOp, from: TypeId, to: TypeId) -> IrResult<()> {
    let (from_scalar, from_shape) = shape(types, from);
    let (to_scalar, to_shape) = shape(types, to);
    let scalable = |s: Option<(u32, bool)>| s.is_some_and(|(_, scalable)| scalable);
    let castable = |ty| types.is_int(ty) || types.is_float(ty) || types.is_ptr(ty);
    let widths = (types.int_bits(from_scalar), types.int_bits(to_scalar));
    let valid = if op == ConstOp::BitCast {
        castable(from_scalar)
            && castable(to_scalar)
            && scalable(from_shape) == scalable(to_shape)
            && types.size_in_bits_of(from) == types.size_in_bits_of(to)
            && if types.is_ptr(from_scalar) || types.is_ptr(to_scalar) {
                from_scalar == to_scalar && from_shape == to_shape
            } else {
                true
            }
    } else if from_shape != to_shape {
        false
    } else {
        match op {
            ConstOp::Trunc => matches!(widths, (Some(f), Some(t)) if f > t),
            ConstOp::ZExt | ConstOp::SExt => matches!(widths, (Some(f), Some(t)) if f < t),
            ConstOp::PtrToInt | ConstOp::PtrToAddr => {
                types.is_ptr(from_scalar) && widths.1.is_some()
            }
            ConstOp::IntToPtr => widths.0.is_some() && types.is_ptr(to_scalar),
            ConstOp::AddrSpaceCast => types.is_ptr(from_scalar) && types.is_ptr(to_scalar),
            _ => unreachable!("is_cast covers exactly these operators"),
        }
    };
    if valid {
        Ok(())
    } else {
        err(IrErrorKind::TypeMismatch, format!("invalid {} between these types", op.name()))
    }
}

#[cfg(test)]
// The tests name the two tables `t` and `c` and symbols `f` and `g`.
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;
    use crate::module::Module;
    use crate::target::TargetSpec;

    fn module() -> Module {
        Module::new(TargetSpec::x86_64_linux_gnu())
    }

    fn kind_of(result: IrResult<ConstId>) -> IrErrorKind {
        result.unwrap_err().kind
    }

    fn is_expr(pool: &ConstPool, id: ConstId, expected: ConstOp) -> bool {
        matches!(pool.get(id), Const::Expr { op, .. } if op == expected)
    }

    /// §4.2: a constant GEP is `symbol + offset`.
    #[test]
    fn gep_folds_to_symbol_plus_offset() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let g = SymId::new(7);
        let base = c.addr(g, 0).unwrap();
        let st = t.struct_type(&[TypeId::I8, TypeId::I32, TypeId::I64], false).unwrap();
        let arr = t.array(st, 10).unwrap();
        let i = |c: &mut ConstPool, t: &TypeTable, v: i64| c.int_signed(t, TypeId::I64, v).unwrap();
        let field = |c: &mut ConstPool, t: &TypeTable, v: u64| c.int(t, TypeId::I32, v).unwrap();
        let (zero, three) = (i(c, t, 0), i(c, t, 3));
        let (f1, f2) = (field(c, t, 1), field(c, t, 2));
        // &g[0][3].2 = 3 * 16 + 8
        let folded = c.fold_gep(t, arr, base, &[zero, three, f2]).unwrap();
        assert_eq!(c.get(folded), Const::Addr { sym: g, addend: 56 });
        // GEPs compose.
        let again = c.fold_gep(t, st, folded, &[three, f1]).unwrap();
        assert_eq!(c.get(again), Const::Addr { sym: g, addend: 56 + 48 + 4 });
        // Negative and narrow indices are sign-extended.
        let minus_one = i(c, t, -1);
        let back = c.fold_gep(t, TypeId::I32, base, &[minus_one]).unwrap();
        assert_eq!(c.get(back), Const::Addr { sym: g, addend: -4 });
        let narrow = c.int_signed(t, TypeId::I8, -2).unwrap();
        let back = c.fold_gep(t, TypeId::I32, base, &[narrow]).unwrap();
        assert_eq!(c.get(back), Const::Addr { sym: g, addend: -8 });
        let wide_index = c.int_signed(t, TypeId::I128, -3).unwrap();
        let back = c.fold_gep(t, TypeId::I32, base, &[wide_index]).unwrap();
        assert_eq!(c.get(back), Const::Addr { sym: g, addend: -12 });
        // Offset zero and no indices give the base itself.
        let f0 = field(c, t, 0);
        assert_eq!(c.fold_gep(t, arr, base, &[zero, zero, f0]).unwrap(), base);
        assert_eq!(c.fold_gep(t, arr, base, &[]).unwrap(), base);
        // A vector steps by its element's bytes, an array by its alloc size.
        let i24 = t.int(24).unwrap();
        let v24 = t.vector(i24, 4).unwrap();
        let a24 = t.array(i24, 4).unwrap();
        let in_vector = c.fold_gep(t, v24, base, &[zero, three]).unwrap();
        assert_eq!(c.get(in_vector), Const::Addr { sym: g, addend: 9 });
        let in_array = c.fold_gep(t, a24, base, &[zero, three]).unwrap();
        assert_eq!(c.get(in_array), Const::Addr { sym: g, addend: 12 });
        // Offsets wrap.
        let big = c.addr(g, i64::MAX).unwrap();
        let one = i(c, t, 1);
        let wrapped = c.fold_gep(t, TypeId::I8, big, &[one]).unwrap();
        assert_eq!(c.get(wrapped), Const::Addr { sym: g, addend: i64::MIN });
    }

    /// §4.2: GEPs from `null` and absolute addresses are absolute addresses.
    #[test]
    fn gep_on_absolute_addresses() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let null = c.null(t, TypeId::PTR).unwrap();
        let one = c.int(t, TypeId::I64, 1).unwrap();
        let minus_one = c.int_signed(t, TypeId::I64, -1).unwrap();
        let size = c.fold_gep(t, TypeId::I64, null, &[one]).unwrap();
        assert_eq!(c.get(size), Const::AddrOfInt { value: 8 });
        let more = c.fold_gep(t, TypeId::I32, size, &[one]).unwrap();
        assert_eq!(c.get(more), Const::AddrOfInt { value: 12 });
        let back = c.fold_gep(t, TypeId::I64, size, &[minus_one]).unwrap();
        assert_eq!(back, null);
        let below = c.fold_gep(t, TypeId::I8, null, &[minus_one]).unwrap();
        assert_eq!(c.get(below), Const::AddrOfInt { value: u64::MAX });
        // The idiom for `sizeof`.
        let as_int = c.fold_cast(t, ConstOp::PtrToInt, size, TypeId::I64).unwrap();
        assert_eq!(c.get(as_int), Const::Int { ty: TypeId::I64, value: 8 });
        let poison = c.poison(t, TypeId::PTR).unwrap();
        assert_eq!(c.fold_gep(t, TypeId::I64, poison, &[one]).unwrap(), poison);
        let undef = c.undef(t, TypeId::PTR).unwrap();
        assert_eq!(c.fold_gep(t, TypeId::I64, undef, &[one]).unwrap(), undef);
    }

    /// §4.2: a GEP with no normal form is an `Expr`, not an error.
    #[test]
    fn gep_without_normal_form_is_an_expr() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let g = SymId::new(1);
        let base = c.addr(g, 0).unwrap();
        let one = c.int(t, TypeId::I64, 1).unwrap();
        let gep = ConstOp::GetElementPtr;
        // A non-constant index.
        let unknown = c.undef(t, TypeId::I64).unwrap();
        let e = c.fold_gep(t, TypeId::I32, base, &[unknown]).unwrap();
        assert_eq!(
            c.get(e),
            Const::Expr {
                op: gep,
                ty: TypeId::PTR,
                source_ty: Some(TypeId::I32),
                args: &[base, unknown]
            }
        );
        // A base that is not `symbol + addend`.
        let block = c.block_addr(g, crate::ids::BlockId::new(0)).unwrap();
        let e = c.fold_gep(t, TypeId::I32, block, &[one]).unwrap();
        assert!(is_expr(c, e, gep));
        let as1 = t.ptr(1).unwrap();
        let null1 = c.null(t, as1).unwrap();
        let e = c.fold_gep(t, TypeId::I32, null1, &[one]).unwrap();
        assert!(is_expr(c, e, gep));
        assert_eq!(c.ty(e), as1);
        // A scalable source type.
        let scalable = t.scalable_vector(TypeId::I32, 4).unwrap();
        let e = c.fold_gep(t, scalable, base, &[one]).unwrap();
        assert!(is_expr(c, e, gep));
        // Elements of a vector of `i1` have no address.
        let bits = t.vector(TypeId::I1, 8).unwrap();
        let zero = c.int(t, TypeId::I64, 0).unwrap();
        let e = c.fold_gep(t, bits, base, &[zero, one]).unwrap();
        assert!(is_expr(c, e, gep));
        // A vector of indices gives a vector of pointers.
        let v2 = t.vector(TypeId::I64, 2).unwrap();
        let indices = c.splat(t, v2, one).unwrap();
        let e = c.fold_gep(t, TypeId::I32, base, &[indices]).unwrap();
        assert!(is_expr(c, e, gep));
        assert_eq!(c.ty(e), t.vector(TypeId::PTR, 2).unwrap());
    }

    /// §4.2 errors of `fold_gep`: `TypeMismatch`, `UnsizedType`, `BadIndex`.
    #[test]
    fn gep_errors() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let base = c.addr(SymId::new(1), 0).unwrap();
        let zero = c.int(t, TypeId::I64, 0).unwrap();
        let st = t.struct_type(&[TypeId::I8, TypeId::I32], false).unwrap();
        let number = c.int(t, TypeId::I64, 5).unwrap();
        assert_eq!(kind_of(c.fold_gep(t, st, number, &[zero])), IrErrorKind::TypeMismatch);
        let real = c.f64(1.0).unwrap();
        assert_eq!(kind_of(c.fold_gep(t, st, base, &[real])), IrErrorKind::TypeMismatch);
        let opaque = t.opaque_struct().unwrap();
        let func = t.function(TypeId::VOID, &[], false).unwrap();
        for unsized_ty in [opaque, func, TypeId::VOID] {
            assert_eq!(kind_of(c.fold_gep(t, unsized_ty, base, &[zero])), IrErrorKind::UnsizedType);
        }
        let two = c.int(t, TypeId::I32, 2).unwrap();
        assert_eq!(kind_of(c.fold_gep(t, st, base, &[zero, two])), IrErrorKind::BadIndex);
        let unknown = c.undef(t, TypeId::I32).unwrap();
        assert_eq!(kind_of(c.fold_gep(t, st, base, &[zero, unknown])), IrErrorKind::BadIndex);
        let one = c.int(t, TypeId::I32, 1).unwrap();
        assert_eq!(kind_of(c.fold_gep(t, st, base, &[zero, one, one])), IrErrorKind::BadIndex);
        assert_eq!(kind_of(c.fold_gep(t, TypeId::I32, base, &[zero, zero])), IrErrorKind::BadIndex);
        c.fold_gep(t, st, base, &[zero, one]).unwrap();
    }

    /// §4.2: integer casts are computed at any width.
    #[test]
    fn integer_casts() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let minus_two = c.int_signed(t, TypeId::I16, -2).unwrap();
        let z = c.fold_cast(t, ConstOp::ZExt, minus_two, TypeId::I64).unwrap();
        assert_eq!(c.get(z), Const::Int { ty: TypeId::I64, value: 0xfffe });
        let s = c.fold_cast(t, ConstOp::SExt, minus_two, TypeId::I64).unwrap();
        assert_eq!(s, c.int_signed(t, TypeId::I64, -2).unwrap());
        let s = c.fold_cast(t, ConstOp::SExt, minus_two, TypeId::I128).unwrap();
        assert_eq!(s, c.int_signed(t, TypeId::I128, -2).unwrap());
        let z = c.fold_cast(t, ConstOp::ZExt, minus_two, TypeId::I128).unwrap();
        assert_eq!(c.get(z), Const::WideInt { ty: TypeId::I128, words: &[0xfffe, 0] });
        let wide_value =
            c.int_from_words(t, TypeId::I128, &[0x1234_5678_9abc_def0, 0xffff]).unwrap();
        let low = c.fold_cast(t, ConstOp::Trunc, wide_value, TypeId::I32).unwrap();
        assert_eq!(c.get(low), Const::Int { ty: TypeId::I32, value: 0x9abc_def0 });
        let i72 = t.int(72).unwrap();
        let mid = c.fold_cast(t, ConstOp::Trunc, wide_value, i72).unwrap();
        assert_eq!(c.get(mid), Const::WideInt { ty: i72, words: &[0x1234_5678_9abc_def0, 0xff] });
        let i200 = t.int(200).unwrap();
        let wider = c.fold_cast(t, ConstOp::SExt, mid, i200).unwrap();
        assert_eq!(
            c.get(wider),
            Const::WideInt { ty: i200, words: &[0x1234_5678_9abc_def0, u64::MAX, u64::MAX, 0xff] }
        );
        let truth = c.int(t, TypeId::I1, 1).unwrap();
        let s = c.fold_cast(t, ConstOp::SExt, truth, TypeId::I8).unwrap();
        assert_eq!(c.get(s), Const::Int { ty: TypeId::I8, value: 255 });
        // undef and poison.
        let undef = c.undef(t, TypeId::I32).unwrap();
        let poison = c.poison(t, TypeId::I32).unwrap();
        assert_eq!(
            c.fold_cast(t, ConstOp::ZExt, undef, TypeId::I64).unwrap(),
            c.zero(t, TypeId::I64).unwrap()
        );
        assert_eq!(
            c.fold_cast(t, ConstOp::Trunc, undef, TypeId::I8).unwrap(),
            c.undef(t, TypeId::I8).unwrap()
        );
        for op in [ConstOp::ZExt, ConstOp::SExt] {
            assert_eq!(
                c.fold_cast(t, op, poison, TypeId::I64).unwrap(),
                c.poison(t, TypeId::I64).unwrap()
            );
        }
    }

    /// §4.2: `ptrtoint`, `inttoptr` and `trunc` on address constants.
    #[test]
    fn address_casts() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let g = SymId::new(3);
        let addr = c.addr(g, 16).unwrap();
        let as_int = c.fold_cast(t, ConstOp::PtrToInt, addr, TypeId::I64).unwrap();
        assert_eq!(c.get(as_int), Const::IntOfAddr { ty: TypeId::I64, addr });
        assert_eq!(c.fold_cast(t, ConstOp::PtrToAddr, addr, TypeId::I64).unwrap(), as_int);
        // inttoptr undoes a full-width ptrtoint.
        assert_eq!(c.fold_cast(t, ConstOp::IntToPtr, as_int, TypeId::PTR).unwrap(), addr);
        // trunc narrows; a narrowed address cannot be widened back.
        let low = c.fold_cast(t, ConstOp::Trunc, as_int, TypeId::I32).unwrap();
        assert_eq!(c.get(low), Const::IntOfAddr { ty: TypeId::I32, addr });
        assert_eq!(c.fold_cast(t, ConstOp::PtrToInt, addr, TypeId::I32).unwrap(), low);
        let e = c.fold_cast(t, ConstOp::IntToPtr, low, TypeId::PTR).unwrap();
        assert!(is_expr(c, e, ConstOp::IntToPtr));
        let e = c.fold_cast(t, ConstOp::ZExt, low, TypeId::I64).unwrap();
        assert!(is_expr(c, e, ConstOp::ZExt));
        let e = c.fold_cast(t, ConstOp::SExt, low, TypeId::I64).unwrap();
        assert!(is_expr(c, e, ConstOp::SExt));
        // zext of the whole address is still the address.
        let wide_int = c.fold_cast(t, ConstOp::ZExt, as_int, TypeId::I128).unwrap();
        assert_eq!(c.get(wide_int), Const::IntOfAddr { ty: TypeId::I128, addr });
        assert_eq!(c.fold_cast(t, ConstOp::Trunc, wide_int, TypeId::I64).unwrap(), as_int);
        // Absolute addresses and null are numbers.
        let null = c.null(t, TypeId::PTR).unwrap();
        let zero = c.fold_cast(t, ConstOp::PtrToInt, null, TypeId::I64).unwrap();
        assert_eq!(c.get(zero), Const::Int { ty: TypeId::I64, value: 0 });
        assert_eq!(c.fold_cast(t, ConstOp::IntToPtr, zero, TypeId::PTR).unwrap(), null);
        let number = c.int(t, TypeId::I64, 0x1234).unwrap();
        let absolute = c.fold_cast(t, ConstOp::IntToPtr, number, TypeId::PTR).unwrap();
        assert_eq!(c.get(absolute), Const::AddrOfInt { value: 0x1234 });
        assert_eq!(c.fold_cast(t, ConstOp::PtrToInt, absolute, TypeId::I64).unwrap(), number);
        let truncated = c.fold_cast(t, ConstOp::PtrToInt, absolute, TypeId::I8).unwrap();
        assert_eq!(c.get(truncated), Const::Int { ty: TypeId::I8, value: 0x34 });
        // inttoptr zero-extends a narrow integer and truncates a wide one.
        let narrow = c.int_signed(t, TypeId::I8, -1).unwrap();
        let p = c.fold_cast(t, ConstOp::IntToPtr, narrow, TypeId::PTR).unwrap();
        assert_eq!(c.get(p), Const::AddrOfInt { value: 255 });
        let huge = c.int_from_words(t, TypeId::I128, &[77, 99]).unwrap();
        let p = c.fold_cast(t, ConstOp::IntToPtr, huge, TypeId::PTR).unwrap();
        assert_eq!(c.get(p), Const::AddrOfInt { value: 77 });
        // Other address constants.
        let dso = c.dso_local_equivalent(g).unwrap();
        let int = c.fold_cast(t, ConstOp::PtrToInt, dso, TypeId::I64).unwrap();
        assert_eq!(c.get(int), Const::IntOfAddr { ty: TypeId::I64, addr: dso });
        // Other address spaces keep the expression, except for zero.
        let as1 = t.ptr(1).unwrap();
        let e = c.fold_cast(t, ConstOp::IntToPtr, number, as1).unwrap();
        assert!(is_expr(c, e, ConstOp::IntToPtr));
        assert_eq!(c.fold_cast(t, ConstOp::IntToPtr, zero, as1).unwrap(), c.null(t, as1).unwrap());
        let e = c.fold_cast(t, ConstOp::AddrSpaceCast, addr, as1).unwrap();
        assert_eq!(
            c.get(e),
            Const::Expr { op: ConstOp::AddrSpaceCast, ty: as1, source_ty: None, args: &[addr] }
        );
        let e = c.fold_cast(t, ConstOp::AddrSpaceCast, null, as1).unwrap();
        assert!(is_expr(c, e, ConstOp::AddrSpaceCast));
    }

    /// §4.2: `bitcast` is the identity or a reinterpretation of the bits.
    #[test]
    fn bitcasts() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let addr = c.addr(SymId::new(1), 0).unwrap();
        assert_eq!(c.fold_cast(t, ConstOp::BitCast, addr, TypeId::PTR).unwrap(), addr);
        let one = c.f64(1.0).unwrap();
        let bits = c.fold_cast(t, ConstOp::BitCast, one, TypeId::I64).unwrap();
        assert_eq!(c.get(bits), Const::Int { ty: TypeId::I64, value: 0x3ff0_0000_0000_0000 });
        assert_eq!(c.fold_cast(t, ConstOp::BitCast, bits, TypeId::DOUBLE).unwrap(), one);
        let quad = c.float(t, TypeId::FP128, (0x4000 << 112) | 5).unwrap();
        let bits = c.fold_cast(t, ConstOp::BitCast, quad, TypeId::I128).unwrap();
        assert_eq!(c.get(bits), Const::WideInt { ty: TypeId::I128, words: &[5, 0x4000 << 48] });
        assert_eq!(c.fold_cast(t, ConstOp::BitCast, bits, TypeId::FP128).unwrap(), quad);
        let i80 = t.int(80).unwrap();
        let x87 = c.float(t, TypeId::X86_FP80, 0x3fff_8000_0000_0000_0000).unwrap();
        let bits = c.fold_cast(t, ConstOp::BitCast, x87, i80).unwrap();
        assert_eq!(
            c.get(bits),
            Const::WideInt { ty: i80, words: &[0x8000_0000_0000_0000, 0x3fff] }
        );
        assert_eq!(c.fold_cast(t, ConstOp::BitCast, bits, TypeId::X86_FP80).unwrap(), x87);
        let half = c.float(t, TypeId::HALF, 0x3c00).unwrap();
        let brain = c.fold_cast(t, ConstOp::BitCast, half, TypeId::BFLOAT).unwrap();
        assert_eq!(c.get(brain), Const::Float { ty: TypeId::BFLOAT, bits: 0x3c00 });
        // Vectors: zero folds, the rest stays.
        let v2 = t.vector(TypeId::I32, 2).unwrap();
        let zero = c.zero(t, v2).unwrap();
        assert_eq!(
            c.fold_cast(t, ConstOp::BitCast, zero, TypeId::I64).unwrap(),
            c.zero(t, TypeId::I64).unwrap()
        );
        let ones = c.int(t, TypeId::I32, 1).unwrap();
        let splat = c.splat(t, v2, ones).unwrap();
        let e = c.fold_cast(t, ConstOp::BitCast, splat, TypeId::I64).unwrap();
        assert!(is_expr(c, e, ConstOp::BitCast));
        let v2x64 = t.vector(TypeId::I64, 2).unwrap();
        let e = c.fold_cast(t, ConstOp::ZExt, splat, v2x64).unwrap();
        assert!(is_expr(c, e, ConstOp::ZExt));
        let poison = c.poison(t, v2).unwrap();
        assert_eq!(
            c.fold_cast(t, ConstOp::ZExt, poison, v2x64).unwrap(),
            c.poison(t, v2x64).unwrap()
        );
    }

    /// §4.2 errors of `fold_cast`: `InvalidOperand` and `TypeMismatch`.
    #[test]
    fn cast_errors() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let int = c.int(t, TypeId::I32, 1).unwrap();
        let addr = c.addr(SymId::new(1), 0).unwrap();
        let real = c.f64(1.0).unwrap();
        let v2 = t.vector(TypeId::I32, 2).unwrap();
        let v4 = t.vector(TypeId::I64, 4).unwrap();
        let v2f = t.vector(TypeId::FLOAT, 2).unwrap();
        let as1 = t.ptr(1).unwrap();
        let st = t.struct_type(&[TypeId::I32], false).unwrap();
        let vector = c.undef(t, v2).unwrap();
        let aggregate = c.undef(t, st).unwrap();
        for op in [ConstOp::Add, ConstOp::GetElementPtr, ConstOp::ExtractElement] {
            assert_eq!(kind_of(c.fold_cast(t, op, int, TypeId::I64)), IrErrorKind::InvalidOperand);
        }
        let bad = [
            (ConstOp::Trunc, int, TypeId::I64),
            (ConstOp::Trunc, int, TypeId::I32),
            (ConstOp::Trunc, real, TypeId::I32),
            (ConstOp::ZExt, int, TypeId::I8),
            (ConstOp::SExt, int, TypeId::I32),
            (ConstOp::ZExt, int, TypeId::DOUBLE),
            (ConstOp::ZExt, vector, TypeId::I64),
            (ConstOp::ZExt, vector, v4),
            (ConstOp::PtrToInt, int, TypeId::I64),
            (ConstOp::PtrToInt, addr, TypeId::DOUBLE),
            (ConstOp::PtrToAddr, addr, TypeId::PTR),
            (ConstOp::IntToPtr, addr, TypeId::PTR),
            (ConstOp::IntToPtr, int, TypeId::I64),
            (ConstOp::IntToPtr, real, TypeId::PTR),
            (ConstOp::BitCast, int, TypeId::I64),
            (ConstOp::BitCast, int, TypeId::DOUBLE),
            (ConstOp::BitCast, addr, TypeId::I64),
            (ConstOp::BitCast, addr, as1),
            (ConstOp::BitCast, aggregate, TypeId::I32),
            (ConstOp::BitCast, vector, TypeId::I32),
            (ConstOp::AddrSpaceCast, int, as1),
            (ConstOp::AddrSpaceCast, addr, TypeId::I64),
        ];
        for (op, value, to) in bad {
            assert_eq!(kind_of(c.fold_cast(t, op, value, to)), IrErrorKind::TypeMismatch, "{op:?}");
        }
        // The valid neighbours of the cases above.
        c.fold_cast(t, ConstOp::BitCast, vector, TypeId::I64).unwrap();
        c.fold_cast(t, ConstOp::BitCast, vector, v2f).unwrap();
        c.fold_cast(t, ConstOp::BitCast, int, TypeId::FLOAT).unwrap();
    }

    /// §4.2: integer arithmetic at any width.
    #[test]
    fn integer_arithmetic() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let int = |c: &mut ConstPool, t: &TypeTable, ty, v| c.int_signed(t, ty, v).unwrap();
        let (a, b) = (int(c, t, TypeId::I8, 0x64), int(c, t, TypeId::I8, 0x64));
        let value = |c: &mut ConstPool, t: &TypeTable, op, a, b| {
            let id = c.fold_binary(t, op, a, b).unwrap();
            c.int_value(id).unwrap()
        };
        assert_eq!(value(c, t, ConstOp::Add, a, b), 200);
        assert_eq!(value(c, t, ConstOp::Mul, a, b), 0x2710 & 0xff);
        assert_eq!(value(c, t, ConstOp::Sub, a, b), 0);
        let three = int(c, t, TypeId::I8, 3);
        assert_eq!(value(c, t, ConstOp::Sub, three, a), (3u64.wrapping_sub(100)) & 0xff);
        assert_eq!(value(c, t, ConstOp::Xor, three, a), 3 ^ 0x64);
        assert_eq!(value(c, t, ConstOp::And, three, a), 3 & 0x64);
        assert_eq!(value(c, t, ConstOp::Or, three, a), 3 | 0x64);
        assert_eq!(value(c, t, ConstOp::Shl, a, three), (100 << 3) & 0xff);
        let eight = int(c, t, TypeId::I8, 8);
        assert_eq!(
            c.fold_binary(t, ConstOp::Shl, a, eight).unwrap(),
            c.poison(t, TypeId::I8).unwrap()
        );
        let (x, y) = (int(c, t, TypeId::I64, -5), int(c, t, TypeId::I64, 7));
        assert_eq!(value(c, t, ConstOp::Mul, x, y), (-35i64) as u64);
        // Wide.
        let (p, q) = (int(c, t, TypeId::I128, -1), int(c, t, TypeId::I128, 2));
        let sum = c.fold_binary(t, ConstOp::Add, p, q).unwrap();
        assert_eq!(sum, c.int(t, TypeId::I128, 1).unwrap());
        let product = c.fold_binary(t, ConstOp::Mul, p, q).unwrap();
        assert_eq!(product, int(c, t, TypeId::I128, -2));
        let seventy = c.int(t, TypeId::I128, 70).unwrap();
        let shifted = c.fold_binary(t, ConstOp::Shl, q, seventy).unwrap();
        assert_eq!(c.get(shifted), Const::WideInt { ty: TypeId::I128, words: &[0, 1 << 7] });
        let too_far = c.int_from_words(t, TypeId::I128, &[0, 1]).unwrap();
        assert_eq!(
            c.fold_binary(t, ConstOp::Shl, q, too_far).unwrap(),
            c.poison(t, TypeId::I128).unwrap()
        );
        // poison propagates.
        let poison = c.poison(t, TypeId::I8).unwrap();
        assert_eq!(c.fold_binary(t, ConstOp::Add, a, poison).unwrap(), poison);
        assert_eq!(c.fold_binary(t, ConstOp::Mul, poison, a).unwrap(), poison);
        // undef is not folded.
        let undef = c.undef(t, TypeId::I8).unwrap();
        let e = c.fold_binary(t, ConstOp::Add, a, undef).unwrap();
        assert_eq!(
            c.get(e),
            Const::Expr { op: ConstOp::Add, ty: TypeId::I8, source_ty: None, args: &[a, undef] }
        );
    }

    /// §4.2: `sub (ptrtoint a), (ptrtoint b)` and friends become `SymDiff`.
    #[test]
    fn symbol_arithmetic() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let (a, b) = (SymId::new(1), SymId::new(2));
        let int_of = |c: &mut ConstPool, t: &TypeTable, sym, addend, ty| {
            let addr = c.addr(sym, addend).unwrap();
            c.fold_cast(t, ConstOp::PtrToInt, addr, ty).unwrap()
        };
        let (pa, pb) = (int_of(c, t, a, 8, TypeId::I64), int_of(c, t, b, 2, TypeId::I64));
        let diff = c.fold_binary(t, ConstOp::Sub, pa, pb).unwrap();
        assert_eq!(c.get(diff), Const::SymDiff { ty: TypeId::I64, a, b, addend: 6, plt: false });
        // Constants on either side, added or subtracted.
        let four = c.int(t, TypeId::I64, 4).unwrap();
        let plus = c.fold_binary(t, ConstOp::Add, diff, four).unwrap();
        assert_eq!(c.get(plus), Const::SymDiff { ty: TypeId::I64, a, b, addend: 10, plt: false });
        assert_eq!(c.fold_binary(t, ConstOp::Add, four, diff).unwrap(), plus);
        let minus = c.fold_binary(t, ConstOp::Sub, diff, four).unwrap();
        assert_eq!(c.get(minus), Const::SymDiff { ty: TypeId::I64, a, b, addend: 2, plt: false });
        // The relative-vtable shape: trunc (sub (ptrtoint, ptrtoint)) to i32.
        let narrow = c.fold_cast(t, ConstOp::Trunc, diff, TypeId::I32).unwrap();
        assert_eq!(c.get(narrow), Const::SymDiff { ty: TypeId::I32, a, b, addend: 6, plt: false });
        let four32 = c.int(t, TypeId::I32, 4).unwrap();
        let narrow_minus = c.fold_binary(t, ConstOp::Sub, narrow, four32).unwrap();
        assert_eq!(
            c.get(narrow_minus),
            Const::SymDiff { ty: TypeId::I32, a, b, addend: 2, plt: false }
        );
        let ten32 = c.int(t, TypeId::I32, 10).unwrap();
        let negative = c.fold_binary(t, ConstOp::Sub, narrow, ten32).unwrap();
        assert_eq!(
            c.get(negative),
            Const::SymDiff { ty: TypeId::I32, a, b, addend: -4, plt: false }
        );
        // ptrtoint plus or minus a constant moves the addend.
        let moved = c.fold_binary(t, ConstOp::Add, pa, four).unwrap();
        assert_eq!(moved, int_of(c, t, a, 12, TypeId::I64));
        assert_eq!(c.fold_binary(t, ConstOp::Add, four, pa).unwrap(), moved);
        assert_eq!(
            c.fold_binary(t, ConstOp::Sub, pa, four).unwrap(),
            int_of(c, t, a, 4, TypeId::I64)
        );
        // A symbol minus itself is a number; a difference minus a symbol
        // cancels.
        let same = int_of(c, t, a, 1, TypeId::I64);
        let number = c.fold_binary(t, ConstOp::Sub, pa, same).unwrap();
        assert_eq!(c.get(number), Const::Int { ty: TypeId::I64, value: 7 });
        let back = c.fold_binary(t, ConstOp::Add, diff, pb).unwrap();
        assert!(is_expr(c, back, ConstOp::Add));
        // dso_local_equivalent on the left makes a PLT difference.
        let dso = c.dso_local_equivalent(a).unwrap();
        let dso_int = c.fold_cast(t, ConstOp::PtrToInt, dso, TypeId::I64).unwrap();
        let plt = c.fold_binary(t, ConstOp::Sub, dso_int, pb).unwrap();
        assert_eq!(c.get(plt), Const::SymDiff { ty: TypeId::I64, a, b, addend: -2, plt: true });
        let zero = c.int(t, TypeId::I64, 0).unwrap();
        assert_eq!(c.fold_binary(t, ConstOp::Add, dso_int, zero).unwrap(), dso_int);
        // No normal form: these stay expressions.
        for (op, l, r) in [
            (ConstOp::Add, pa, pb),
            (ConstOp::Sub, four, pa),
            (ConstOp::Sub, pa, dso_int),
            (ConstOp::Add, dso_int, four),
            (ConstOp::Sub, diff, diff),
            (ConstOp::Mul, pa, four),
            (ConstOp::Xor, pa, four),
            (ConstOp::Shl, pa, four),
        ] {
            let e = c.fold_binary(t, op, l, r).unwrap();
            assert_eq!(
                c.get(e),
                Const::Expr { op, ty: TypeId::I64, source_ty: None, args: &[l, r] }
            );
        }
        // Wider than 64 bits there is no normal form either.
        let wide_a = int_of(c, t, a, 0, TypeId::I128);
        let wide_b = int_of(c, t, b, 0, TypeId::I128);
        let e = c.fold_binary(t, ConstOp::Sub, wide_a, wide_b).unwrap();
        assert!(is_expr(c, e, ConstOp::Sub));
    }

    /// §4.2 errors of `fold_binary`: `InvalidOperand` and `TypeMismatch`.
    #[test]
    fn binary_errors() {
        let mut m = module();
        let (t, c) = (&mut m.types, &mut m.consts);
        let (a, b) = (c.int(t, TypeId::I32, 1).unwrap(), c.int(t, TypeId::I64, 1).unwrap());
        let addr = c.addr(SymId::new(1), 0).unwrap();
        let real = c.f64(1.0).unwrap();
        for op in [ConstOp::Trunc, ConstOp::GetElementPtr, ConstOp::ShuffleVector] {
            assert_eq!(kind_of(c.fold_binary(t, op, a, a)), IrErrorKind::InvalidOperand);
        }
        assert_eq!(kind_of(c.fold_binary(t, ConstOp::Add, a, b)), IrErrorKind::TypeMismatch);
        assert_eq!(kind_of(c.fold_binary(t, ConstOp::Sub, addr, addr)), IrErrorKind::TypeMismatch);
        assert_eq!(kind_of(c.fold_binary(t, ConstOp::Mul, real, real)), IrErrorKind::TypeMismatch);
        let v2 = t.vector(TypeId::I32, 2).unwrap();
        let splat = c.splat(t, v2, a).unwrap();
        assert_eq!(kind_of(c.fold_binary(t, ConstOp::Add, splat, a)), IrErrorKind::TypeMismatch);
        let e = c.fold_binary(t, ConstOp::Add, splat, splat).unwrap();
        assert!(is_expr(c, e, ConstOp::Add));
    }
}
