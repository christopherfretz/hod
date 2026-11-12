//! The interpreter's model of C++ exceptions (`spec/interp.md` §5.9): the
//! names of the two functions it provides, the personality and the
//! type-information objects it knows, and their declarations.
//!
//! An exception starts with a call of [`THROW`], `void hod_throw(ptr type,
//! i64 payload)`: `type` is the address of a type-information object
//! (`@_ZTIi`, ...), which is the exception's type identity, and `payload`
//! is what [`CATCH`], `i64 hod_catch(ptr exception)`, returns for it in a
//! landing pad. On real hardware the two are a few lines of C++ over the
//! Itanium ABI (`__cxa_allocate_exception` and `__cxa_throw`;
//! `__cxa_begin_catch` and `__cxa_end_catch`): the LLVM oracle links
//! exactly that (§10.2).

use hod_ir::{FunctionDesc, IrResult, Module, SymId, TypeId, VariableDesc};

/// `void hod_throw(ptr type, i64 payload)`: throws an exception of type
/// `type` carrying `payload`.
pub const THROW: &str = "hod_throw";
/// `i64 hod_catch(ptr exception)`: catches the exception a landing pad
/// received and returns its payload. The exception is finished.
pub const CATCH: &str = "hod_catch";
/// The one personality function the model knows: the Itanium C++ one.
pub const PERSONALITY: &str = "__gxx_personality_v0";
/// Type-information objects of fundamental types of the C++ runtime:
/// `int`, `long`, `unsigned int`, `short`. For these the Itanium
/// personality's type match is identity, so it is the model's (§5.9).
pub const TYPE_INFOS: [&str; 4] = ["_ZTIi", "_ZTIl", "_ZTIj", "_ZTIs"];

/// The declarations [`declare`] adds.
#[derive(Clone, Copy, Debug)]
pub struct Eh {
    /// [`THROW`] and its function type.
    pub throw: SymId,
    pub throw_ty: TypeId,
    /// [`CATCH`] and its function type.
    pub catch: SymId,
    pub catch_ty: TypeId,
    /// [`PERSONALITY`], for `FunctionDesc::personality`.
    pub personality: SymId,
    /// The [`TYPE_INFOS`], declared as external constants.
    pub type_infos: [SymId; 4],
    /// `{ ptr, i32 }`: the type of a landing pad.
    pub pad_ty: TypeId,
}

/// Declares the throwing and catching functions, the personality and the
/// type-information objects in `module`.
///
/// # Errors
/// What `hod_ir` refuses, for instance a name already declared otherwise.
pub fn declare(module: &mut Module) -> IrResult<Eh> {
    let throw_ty = module.types.function(TypeId::VOID, &[TypeId::PTR, TypeId::I64], false)?;
    let catch_ty = module.types.function(TypeId::I64, &[TypeId::PTR], false)?;
    let personality_ty = module.types.function(TypeId::I32, &[], true)?;
    let throw = module.add_function(THROW.as_bytes(), &FunctionDesc::new(throw_ty))?;
    let catch = module.add_function(CATCH.as_bytes(), &FunctionDesc::new(catch_ty))?;
    let personality =
        module.add_function(PERSONALITY.as_bytes(), &FunctionDesc::new(personality_ty))?;
    let mut type_infos = [throw; 4];
    for (sym, name) in type_infos.iter_mut().zip(TYPE_INFOS) {
        let desc = VariableDesc { constant: true, ..VariableDesc::new(TypeId::PTR) };
        *sym = module.add_variable(name.as_bytes(), &desc)?;
    }
    let pad_ty = module.types.struct_type(&[TypeId::PTR, TypeId::I32], false)?;
    Ok(Eh { throw, throw_ty, catch, catch_ty, personality, type_infos, pad_ty })
}
