//! The producer kit (`spec/ir.md` "Producer kit", design 0004 H3): the
//! mapping from LLVM's constructs to `hod_ir` that every producer makes
//! the same way, written once. `hod_ll` and `hod_bc` call it; a third
//! producer (a front end lowering into `hod_ir` directly) calls it the
//! same way, and gets what the readers get.
//!
//! Every helper is cheap on the hot path: no boxing, no allocation per
//! call (the scratch vectors are the caller's), and the generic ones are
//! inlined into the caller.

use crate::{
    Const, ConstId, ConstPool, FunctionBuilder, IrResult, Layering, Module, TypeId, TypeKind,
    TypeTable, Value, sext64,
};

/// What a lowering made of a construct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lowered {
    /// The value, built.
    Built(Value),
    /// `hod_ir` cannot represent the construct: the producer records
    /// `construct` (`Module::record_unsupported`, against the function)
    /// and uses `stand_in` for the result, in a body that will not be
    /// kept.
    Refused { construct: &'static str, stand_in: Value },
}

impl Lowered {
    /// The value, built or standing in.
    #[inline]
    pub fn value(self) -> Value {
        match self {
            Lowered::Built(value) | Lowered::Refused { stand_in: value, .. } => value,
        }
    }

    /// The construct to record, if refused.
    #[inline]
    pub fn refused(self) -> Option<&'static str> {
        match self {
            Lowered::Built(_) => None,
            Lowered::Refused { construct, .. } => Some(construct),
        }
    }
}

/// What stands in for the result of type `ty` of an instruction `hod_ir`
/// cannot represent: no value for `void`, `none` for `token`, `poison`
/// for any other type.
///
/// # Errors
/// As [`ConstPool::poison`].
pub fn stand_in<L: Layering>(module: &mut Module<L>, ty: TypeId) -> IrResult<Value> {
    let id = match ty {
        TypeId::VOID => return Ok(Value::NONE),
        TypeId::TOKEN => module.consts.none_token(),
        _ => module.consts.poison(&module.types, ty),
    };
    Value::constant(id?)
}

// ---- getelementptr ----

/// Why a `getelementptr` is malformed (LLVM's parser refuses it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GepError {
    /// The base (or each element of a vector base) is not a pointer.
    BaseNotPointer,
    /// The source element type is not sized.
    SourceUnsized,
    /// An index (or its element) is not an integer.
    IndexNotInteger,
    /// A vector index's length differs from the base's or an earlier
    /// index's.
    VectorLength,
    /// A struct index that is not a constant in range, or an index into a
    /// type that has no elements.
    InvalidIndices,
}

impl GepError {
    /// LLVM's message for the error.
    pub fn message(self) -> &'static str {
        match self {
            GepError::BaseNotPointer => "base of getelementptr must be a pointer",
            GepError::SourceUnsized => "base element of getelementptr must be sized",
            GepError::IndexNotInteger => "getelementptr index must be an integer",
            GepError::VectorLength => "getelementptr vector index has a wrong number of elements",
            GepError::InvalidIndices => "invalid getelementptr indices",
        }
    }
}

/// The length of a vector type and whether it is scalable.
type Shape = Option<(u32, bool)>;

fn shape<L: Layering>(types: &TypeTable<L>, ty: TypeId) -> (TypeId, Shape) {
    match types.kind(ty) {
        TypeKind::Vector { elem, len } => (elem, Some((len, false))),
        TypeKind::ScalableVector { elem, min_len } => (elem, Some((min_len, true))),
        _ => (ty, None),
    }
}

/// The value of a constant used as a `getelementptr` index: sign-extended
/// or truncated to 64 bits. `None` if it is not one integer (the same in
/// every element, for a vector).
pub fn gep_index_value<L: Layering>(
    types: &TypeTable<L>,
    consts: &ConstPool<L>,
    id: ConstId,
) -> Option<i64> {
    match consts.get(id) {
        Const::Int { ty, value } => Some(sext64(value, types.int_bits(ty).unwrap_or(64))),
        Const::WideInt { words, .. } => Some(words[0] as i64),
        Const::Zero(_) => Some(0),
        Const::Splat { elem, .. } => gep_index_value(types, consts, elem),
        Const::Aggregate { elems, .. } => {
            let first = gep_index_value(types, consts, *elems.first()?)?;
            elems[1..]
                .iter()
                .all(|&elem| gep_index_value(types, consts, elem) == Some(first))
                .then_some(first)
        }
        _ => None,
    }
}

/// One `getelementptr` lowered to a `PtrAdd` (`spec/ir.md` §5.4), index
/// by index, as the producer reads them: [`Gep::new`] with the source
/// element type and the base's type, then for each index
/// [`Gep::index_type`] and [`Gep::index`], then [`Gep::build`].
///
/// Every constant index is folded into the byte offset (wrapping, in 64
/// bits); a variable one is pushed on the caller's list with its scale.
/// `inbounds`, `nuw` and `nusw` are not kept.
#[derive(Clone, Copy, Debug)]
pub struct Gep {
    pointer: TypeId,
    shape: Shape,
    current: TypeId,
    first: bool,
    offset: i64,
    unsupported: Option<&'static str>,
}

impl Gep {
    /// Starts a `getelementptr` over `source` from a base of type
    /// `base_ty` (a pointer, or a vector of pointers).
    ///
    /// # Errors
    /// `BaseNotPointer`, then `SourceUnsized`.
    #[inline]
    pub fn new<L: Layering>(
        types: &TypeTable<L>,
        source: TypeId,
        base_ty: TypeId,
    ) -> Result<Gep, GepError> {
        let (pointer, shape) = shape(types, base_ty);
        if !types.is_ptr(pointer) {
            return Err(GepError::BaseNotPointer);
        }
        if !types.is_sized(source) {
            return Err(GepError::SourceUnsized);
        }
        Ok(Gep { pointer, shape, current: source, first: true, offset: 0, unsupported: None })
    }

    /// Checks the type of the next index and returns its width in bits;
    /// a vector index makes the result a vector.
    ///
    /// # Errors
    /// `IndexNotInteger`, `VectorLength`.
    #[inline]
    pub fn index_type<L: Layering>(
        &mut self,
        types: &TypeTable<L>,
        index_ty: TypeId,
    ) -> Result<u32, GepError> {
        let (scalar, index_shape) = shape(types, index_ty);
        let Some(bits) = types.int_bits(scalar) else {
            return Err(GepError::IndexNotInteger);
        };
        if index_shape.is_some() {
            if self.shape.is_some() && self.shape != index_shape {
                return Err(GepError::VectorLength);
            }
            self.shape = index_shape;
        }
        Ok(bits)
    }

    /// The next index: `value`, whose constant value (if it is one, as
    /// [`gep_index_value`] reads it) is `constant`. A variable index is
    /// pushed on `indices` with its scale.
    ///
    /// # Errors
    /// `InvalidIndices`.
    #[inline]
    pub fn index<L: Layering>(
        &mut self,
        types: &TypeTable<L>,
        value: Value,
        constant: Option<i64>,
        indices: &mut Vec<(Value, u64)>,
    ) -> Result<(), GepError> {
        let current = self.current;
        let stride = if self.first {
            self.first = false;
            (!types.is_scalable(current)).then(|| types.alloc_size_of(current))
        } else {
            match types.kind(current) {
                TypeKind::Struct { fields, .. } => {
                    let field = constant.and_then(|index| {
                        usize::try_from(index).ok().filter(|&index| index < fields.len())
                    });
                    let Some(field) = field else {
                        return Err(GepError::InvalidIndices);
                    };
                    let field_ty = fields[field];
                    if types.is_scalable(current) {
                        self.unsupported = Some("getelementptr:scalable");
                    } else {
                        let at_field = types.field_offset(current, field as u32) as i64;
                        self.offset = self.offset.wrapping_add(at_field);
                    }
                    self.current = field_ty;
                    return Ok(());
                }
                TypeKind::Array { elem, .. } => {
                    self.current = elem;
                    (!types.is_scalable(elem)).then(|| types.alloc_size_of(elem))
                }
                TypeKind::Vector { elem, .. } => {
                    let stride = types.element_stride(current);
                    if stride.is_none() {
                        self.unsupported = Some("getelementptr:vector-element-not-bytes");
                    }
                    self.current = elem;
                    stride.or(Some(0))
                }
                TypeKind::ScalableVector { elem, .. } => {
                    self.current = elem;
                    None
                }
                _ => return Err(GepError::InvalidIndices),
            }
        };
        let Some(stride) = stride else {
            self.unsupported = Some("getelementptr:scalable");
            return Ok(());
        };
        match constant {
            Some(index) => {
                self.offset = self.offset.wrapping_add(index.wrapping_mul(stride as i64))
            }
            None => indices.push((value, stride)),
        }
        Ok(())
    }

    /// The byte offset the constant indices add up to so far.
    #[inline]
    pub fn offset(&self) -> i64 {
        self.offset
    }

    /// The type of the result: the base's pointer type, a vector of it if
    /// the base or an index is a vector.
    ///
    /// # Errors
    /// As [`TypeTable::vector`].
    #[inline]
    pub fn result_type<L: Layering>(&self, types: &mut TypeTable<L>) -> IrResult<TypeId> {
        match self.shape {
            None => Ok(self.pointer),
            Some((len, false)) => types.vector(self.pointer, len),
            Some((len, true)) => types.scalable_vector(self.pointer, len),
        }
    }

    /// The `PtrAdd` of `base` with the variable `indices`, or the reason
    /// `hod_ir` cannot represent the address (`getelementptr:scalable`,
    /// `getelementptr:vector-element-not-bytes`) and a stand-in; with the
    /// result's type.
    ///
    /// # Errors
    /// As [`Gep::result_type`], [`stand_in`] and
    /// [`FunctionBuilder::ptr_add`].
    #[inline]
    pub fn build<L: Layering>(
        self,
        module: &mut Module<L>,
        b: &mut FunctionBuilder,
        base: Value,
        indices: &[(Value, u64)],
    ) -> IrResult<(TypeId, Lowered)> {
        let ty = self.result_type(&mut module.types)?;
        if let Some(construct) = self.unsupported {
            return Ok((ty, Lowered::Refused { construct, stand_in: stand_in(module, ty)? }));
        }
        Ok((ty, Lowered::Built(b.ptr_add(ty, base, self.offset, indices)?)))
    }
}
