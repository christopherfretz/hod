//! The TYPE block (`spec/bc.md` §5.2), built into `hod_ir` types.
//!
//! Types are structural in `hod_ir`, so a named struct is its fields; a
//! struct's fields may be named after it (a forward reference), so the
//! table is resolved by a walk with its own stack, as `hod_ll` resolves
//! its named types, and a type that holds itself is refused.

use hod_ir::{Module, TypeId};

use crate::codes::ty;
use crate::error::{BcErrorKind, BcResult, fail, ir};
use crate::raw::{Raw, Rec};

/// The type notes of `spec/ll.md` §5.2, as bits.
pub(crate) const NOTE_PPC_FP128: u8 = 1;
pub(crate) const NOTE_X86_MMX: u8 = 2;
pub(crate) const NOTE_X86_AMX: u8 = 4;
/// Not a type note: a `ptrauth` constant (`const-expr:ptrauth`).
pub(crate) const NOTE_PTRAUTH: u8 = 8;

/// The notes of `bits`, by name.
pub(crate) fn note_names(bits: u8) -> impl Iterator<Item = &'static str> {
    [
        (NOTE_PPC_FP128, "type:ppc_fp128"),
        (NOTE_X86_MMX, "type:x86_mmx"),
        (NOTE_X86_AMX, "type:x86_amx"),
        (NOTE_PTRAUTH, "const-expr:ptrauth"),
    ]
    .into_iter()
    .filter(move |&(bit, _)| bits & bit != 0)
    .map(|(_, name)| name)
}

/// The built table.
#[derive(Debug, Default)]
pub(crate) struct Types {
    /// Bitcode type id to type.
    pub ids: Vec<TypeId>,
    /// The notes a use of each type makes: those of the type as written
    /// in place, which for a named struct is its name alone.
    pub notes: Vec<u8>,
    /// The notes of the named struct definitions, recorded against no
    /// symbol.
    pub defined_notes: u8,
    /// The function types with a parameter whose type makes a note: the
    /// type id and the notes of each parameter.
    pub param_notes: Vec<(u64, Vec<u8>)>,
}

impl Types {
    /// The type of bitcode id `id`.
    pub fn get(&self, id: u64, at: u64) -> BcResult<TypeId> {
        match usize::try_from(id).ok().and_then(|i| self.ids.get(i)) {
            Some(&ty) => Ok(ty),
            None => fail(BcErrorKind::Invalid, at, format!("type id {id} names no type")),
        }
    }

    /// The notes of the parameters of function type `id`, if any has one.
    pub fn param_notes(&self, id: u64) -> Option<&[u8]> {
        self.param_notes.iter().find(|(fid, _)| *fid == id).map(|(_, notes)| &notes[..])
    }

    /// The notes a use of bitcode type `id` makes (0 for an unknown id).
    pub fn notes_of(&self, id: u64) -> u8 {
        usize::try_from(id).ok().and_then(|i| self.notes.get(i)).copied().unwrap_or(0)
    }
}

/// One entry of the table, before it is built.
struct Entry {
    rec: Rec,
    /// For a struct, opaque or target type: whether a `STRUCT_NAME` named it.
    named: bool,
    name: Option<Rec>,
}

/// The bitcode type ids an entry holds, by position.
fn child(entry: &Entry, ops: &[u64], k: usize) -> Option<u64> {
    match entry.rec.code {
        ty::ARRAY | ty::VECTOR => (k == 0).then(|| ops.get(1).copied()).flatten(),
        ty::STRUCT_ANON | ty::STRUCT_NAMED | ty::FUNCTION => ops.get(1 + k).copied(),
        ty::TARGET_TYPE => {
            let count = ops.first().copied().unwrap_or(0);
            if (k as u64) < count { ops.get(1 + k).copied() } else { None }
        }
        _ => None,
    }
}

/// Builds the TYPE block's table into `module`.
#[allow(clippy::too_many_lines)] // The records, then the walk that resolves them.
pub(crate) fn build(raw: &Raw<'_>, module: &mut Module) -> BcResult<Types> {
    const NEW: u8 = 0;
    const OPEN: u8 = 1;
    const DONE: u8 = 2;
    let pool = &raw.pool[..];
    let mut entries: Vec<Entry> = Vec::new();
    let mut name: Option<Rec> = None;
    let mut declared: Option<u64> = None;
    for &rec in &raw.types {
        let ops = rec.ops(pool);
        match rec.code {
            ty::NUMENTRY => {
                let [count] = ops else {
                    return fail(
                        BcErrorKind::Invalid,
                        rec.pos,
                        "a NUMENTRY record without its count",
                    );
                };
                if *count > raw.types.len() as u64 {
                    return fail(
                        BcErrorKind::Invalid,
                        rec.pos,
                        "a NUMENTRY count over the records of the block",
                    );
                }
                declared = Some(*count);
                entries.reserve(*count as usize);
            }
            ty::STRUCT_NAME => name = Some(rec),
            ty::POINTER | ty::FUNCTION_OLD => {
                return fail(
                    BcErrorKind::Unsupported("typed-pointers"),
                    rec.pos,
                    "a typed pointer or pre-3.3 function type",
                );
            }
            ty::VOID
            | ty::FLOAT
            | ty::DOUBLE
            | ty::LABEL
            | ty::OPAQUE
            | ty::INTEGER
            | ty::HALF
            | ty::ARRAY
            | ty::VECTOR
            | ty::X86_FP80
            | ty::FP128
            | ty::PPC_FP128
            | ty::METADATA
            | ty::X86_MMX
            | ty::STRUCT_ANON
            | ty::STRUCT_NAMED
            | ty::FUNCTION
            | ty::TOKEN
            | ty::BFLOAT
            | ty::X86_AMX
            | ty::OPAQUE_POINTER
            | ty::TARGET_TYPE => {
                let takes_name =
                    matches!(rec.code, ty::OPAQUE | ty::STRUCT_NAMED | ty::TARGET_TYPE);
                let name = if takes_name { name.take() } else { None };
                entries.push(Entry { rec, named: name.is_some(), name });
            }
            code => {
                return fail(
                    BcErrorKind::Unsupported("record"),
                    rec.pos,
                    format!("record code {code} in a TYPE block"),
                );
            }
        }
    }
    if declared.is_some_and(|count| count != entries.len() as u64) {
        let at = raw.types.first().map_or(0, |rec| rec.pos);
        return fail(
            BcErrorKind::Invalid,
            at,
            "a TYPE block whose NUMENTRY is not its number of types",
        );
    }
    let count = entries.len();
    let mut out = Types {
        ids: vec![TypeId::VOID; count],
        notes: vec![0; count],
        defined_notes: 0,
        param_notes: Vec::new(),
    };
    let mut state = vec![NEW; count];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut fields: Vec<TypeId> = Vec::new();
    for root in 0..count {
        if state[root] != NEW {
            continue;
        }
        state[root] = OPEN;
        stack.push((root, 0));
        while let Some(&mut (at, ref mut next)) = stack.last_mut() {
            let entry = &entries[at];
            let ops = entry.rec.ops(pool);
            if let Some(id) = child(entry, ops, *next) {
                *next += 1;
                let Some(index) = usize::try_from(id).ok().filter(|&i| i < count) else {
                    return fail(
                        BcErrorKind::Invalid,
                        entry.rec.pos,
                        format!("type id {id} names no type"),
                    );
                };
                match state[index] {
                    DONE => {}
                    OPEN => {
                        return fail(
                            BcErrorKind::Type,
                            entry.rec.pos,
                            "a type that contains itself",
                        );
                    }
                    _ => {
                        state[index] = OPEN;
                        stack.push((index, 0));
                    }
                }
                continue;
            }
            let (built, notes) = build_one(entry, ops, pool, &out, module, &mut fields)?;
            out.ids[at] = built;
            if entry.rec.code == ty::FUNCTION && notes != 0 {
                let params = (1..ops.len().saturating_sub(1))
                    .map(|k| child(entry, ops, k).map_or(0, |id| out.notes_of(id)))
                    .collect();
                out.param_notes.push((at as u64, params));
            }
            if entry.named && matches!(entry.rec.code, ty::STRUCT_NAMED | ty::OPAQUE) {
                out.defined_notes |= notes;
            } else {
                out.notes[at] = notes;
            }
            state[at] = DONE;
            stack.pop();
        }
    }
    Ok(out)
}

/// Builds one entry whose children are built.
fn build_one(
    entry: &Entry,
    ops: &[u64],
    pool: &[u64],
    out: &Types,
    module: &mut Module,
    fields: &mut Vec<TypeId>,
) -> BcResult<(TypeId, u8)> {
    let at = entry.rec.pos;
    let types = &mut module.types;
    let short = || fail(BcErrorKind::Invalid, at, "a type record cut short");
    let u32_of = |value: u64| match u32::try_from(value) {
        Ok(value) => Ok(value),
        Err(_) => fail(BcErrorKind::Invalid, at, "a type field over 32 bits"),
    };
    let children = |k: usize| -> (TypeId, u8) {
        let id = child(entry, ops, k).unwrap_or(0) as usize;
        (out.ids[id], out.notes[id])
    };
    Ok(match entry.rec.code {
        ty::VOID => (TypeId::VOID, 0),
        ty::FLOAT => (TypeId::FLOAT, 0),
        ty::DOUBLE => (TypeId::DOUBLE, 0),
        ty::LABEL => (TypeId::LABEL, 0),
        ty::HALF => (TypeId::HALF, 0),
        ty::BFLOAT => (TypeId::BFLOAT, 0),
        ty::X86_FP80 => (TypeId::X86_FP80, 0),
        ty::FP128 => (TypeId::FP128, 0),
        ty::METADATA => (TypeId::METADATA, 0),
        ty::TOKEN => (TypeId::TOKEN, 0),
        // Not types of `hod_ir`: stood in for as `hod_ll` does, and noted.
        ty::PPC_FP128 => (TypeId::FP128, NOTE_PPC_FP128),
        ty::X86_MMX => (ir(at, types.vector(TypeId::I64, 1))?, NOTE_X86_MMX),
        ty::X86_AMX => (ir(at, types.target_ext("x86_amx"))?, NOTE_X86_AMX),
        ty::INTEGER => {
            let [bits] = ops else { return short() };
            (ir(at, types.int(u32_of(*bits)?))?, 0)
        }
        ty::OPAQUE_POINTER => {
            let space = ops.first().copied().unwrap_or(0);
            (ir(at, types.ptr(u32_of(space)?))?, 0)
        }
        ty::OPAQUE => (ir(at, types.opaque_struct())?, 0),
        ty::ARRAY => {
            let [len, _] = ops else { return short() };
            let (elem, notes) = children(0);
            (ir(at, types.array(elem, *len))?, notes)
        }
        ty::VECTOR => {
            let (len, scalable) = match ops {
                [len, _] => (*len, false),
                [len, _, scalable] => (*len, *scalable != 0),
                _ => return short(),
            };
            let (elem, notes) = children(0);
            let len = u32_of(len)?;
            let built =
                if scalable { types.scalable_vector(elem, len) } else { types.vector(elem, len) };
            (ir(at, built)?, notes)
        }
        ty::STRUCT_ANON | ty::STRUCT_NAMED => {
            let Some((&packed, rest)) = ops.split_first() else { return short() };
            fields.clear();
            let mut notes = 0;
            for k in 0..rest.len() {
                let (field, n) = children(k);
                fields.push(field);
                notes |= n;
            }
            (ir(at, types.struct_type(fields, packed != 0))?, notes)
        }
        ty::FUNCTION => {
            let [vararg, _, params @ ..] = ops else { return short() };
            let (ret, mut notes) = children(0);
            fields.clear();
            for k in 0..params.len() {
                let (param, n) = children(k + 1);
                fields.push(param);
                notes |= n;
            }
            (ir(at, types.function(ret, fields, *vararg != 0))?, notes)
        }
        ty::TARGET_TYPE => {
            let Some(name) = entry.name else {
                return fail(BcErrorKind::Invalid, at, "a target extension type without its name");
            };
            let Some(&count) = ops.first() else { return short() };
            if count > ops.len() as u64 - 1 {
                return short();
            }
            let mut notes = 0;
            for k in 0..count as usize {
                notes |= children(k).1;
            }
            let name = crate::attrs::chars(name.ops(pool), name.pos)?;
            (ir(at, types.target_ext(&String::from_utf8_lossy(&name)))?, notes)
        }
        code => unreachable!("type code {code} is filtered in `build`"),
    })
}
