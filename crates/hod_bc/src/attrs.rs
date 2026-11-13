//! Attributes (`spec/bc.md` §5.3): the `PARAMATTR_GROUP` entries, decoded
//! into what `hod_ll` makes of the same attributes in text — function
//! attributes as a group's, return and parameter attributes as written in
//! place — with the same names noted.

use hod_ir::kit::{
    AttrUse, TypedAttrError, ValueKeyword, add_typed, attr_construct, fn_keyword, string_attr,
    value_keyword,
};
use hod_ir::{AttrSyntax, FnAttrs, FnAttrsId, Layering, ParamAttrs, UwTable, attr_info};

use crate::error::{BcErrorKind, BcResult, fail, ir};

/// The keyword of each `ATTR_KIND_*` code of LLVM 19 to 22 (index = code).
pub(crate) const KIND_NAMES: [&str; 106] = [
    "",
    "align",
    "alwaysinline",
    "byval",
    "inlinehint",
    "inreg",
    "minsize",
    "naked",
    "nest",
    "noalias",
    "nobuiltin",
    "nocapture",
    "noduplicate",
    "noimplicitfloat",
    "noinline",
    "nonlazybind",
    "noredzone",
    "noreturn",
    "nounwind",
    "optsize",
    "readnone",
    "readonly",
    "returned",
    "returns_twice",
    "signext",
    "alignstack",
    "ssp",
    "sspreq",
    "sspstrong",
    "sret",
    "sanitize_address",
    "sanitize_thread",
    "sanitize_memory",
    "uwtable",
    "zeroext",
    "builtin",
    "cold",
    "optnone",
    "inalloca",
    "nonnull",
    "jumptable",
    "dereferenceable",
    "dereferenceable_or_null",
    "convergent",
    "safestack",
    "argmemonly",
    "swiftself",
    "swifterror",
    "norecurse",
    "inaccessiblememonly",
    "inaccessiblemem_or_argmemonly",
    "allocsize",
    "writeonly",
    "speculatable",
    "strictfp",
    "sanitize_hwaddress",
    "nocf_check",
    "optforfuzzing",
    "shadowcallstack",
    "speculative_load_hardening",
    "immarg",
    "willreturn",
    "nofree",
    "nosync",
    "sanitize_memtag",
    "preallocated",
    "nomerge",
    "null_pointer_is_valid",
    "noundef",
    "byref",
    "mustprogress",
    "nocallback",
    "hot",
    "noprofile",
    "vscale_range",
    "swiftasync",
    "nosanitize_coverage",
    "elementtype",
    "disable_sanitizer_instrumentation",
    "nosanitize_bounds",
    "allocalign",
    "allocptr",
    "allockind",
    "presplitcoroutine",
    "fn_ret_thunk_extern",
    "skipprofile",
    "memory",
    "nofpclass",
    "optdebug",
    "writable",
    "coro_only_destroy_when_complete",
    "dead_on_unwind",
    "range",
    "sanitize_numerical_stability",
    "initializes",
    "hybrid_patchable",
    "sanitize_realtime",
    "sanitize_realtime_blocking",
    "coro_elide_safe",
    "noext",
    "nodivergencesource",
    "sanitize_type",
    "captures",
    "dead_on_return",
    "sanitize_alloc_token",
    "nocreateundeforpoison",
];

/// The group index of function attributes.
pub(crate) const FUNCTION_INDEX: u64 = 0xFFFF_FFFF;

/// One attribute of a group entry.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Attr<'r> {
    /// A keyword attribute, with its integer (or 0) and its type (bitcode
    /// type id).
    Keyword { kind: u64, value: Option<u64>, ty: Option<u64> },
    /// A string attribute: key and value as the record's characters.
    Str { key: &'r [u64], value: &'r [u64] },
}

/// The attributes of a group entry's operands after `[grpid, index]`, in
/// order; `at` is the record's position.
pub(crate) fn decode<'r>(ops: &'r [u64], at: u64, out: &mut Vec<Attr<'r>>) -> BcResult<()> {
    out.clear();
    let mut i = 0;
    let short = || fail(BcErrorKind::Invalid, at, "an attribute group entry cut short");
    while i < ops.len() {
        let tag = ops[i];
        let need = |n: usize| if i + n < ops.len() { Ok(()) } else { short() };
        match tag {
            // 5 is a type attribute written without its type.
            0 | 5 => {
                need(1)?;
                out.push(Attr::Keyword { kind: ops[i + 1], value: None, ty: None });
                i += 2;
            }
            1 => {
                need(2)?;
                out.push(Attr::Keyword { kind: ops[i + 1], value: Some(ops[i + 2]), ty: None });
                i += 3;
            }
            3 | 4 => {
                let key_start = i + 1;
                let Some(key_len) = ops[key_start..].iter().position(|&c| c == 0) else {
                    return short();
                };
                let key = &ops[key_start..key_start + key_len];
                i = key_start + key_len + 1;
                let mut value: &[u64] = &[];
                if tag == 4 {
                    let Some(len) = ops[i..].iter().position(|&c| c == 0) else {
                        return short();
                    };
                    value = &ops[i..i + len];
                    i += len + 1;
                }
                out.push(Attr::Str { key, value });
            }
            6 => {
                need(2)?;
                out.push(Attr::Keyword { kind: ops[i + 1], value: None, ty: Some(ops[i + 2]) });
                i += 3;
            }
            7 => {
                // A constant range: [kind, bitwidth, lower, upper], or for
                // a width over 64 [kind, bitwidth, words, lower…, upper…].
                need(2)?;
                let width = ops[i + 2];
                let len = if width > 64 {
                    need(3)?;
                    let packed = ops[i + 3];
                    3 + (packed & 0xFFFF_FFFF) as usize + (packed >> 32) as usize
                } else {
                    4
                };
                need(len)?;
                out.push(Attr::Keyword { kind: ops[i + 1], value: None, ty: None });
                i += len + 1;
            }
            8 => {
                // A list of constant ranges: [kind, count, bitwidth, ranges…].
                need(3)?;
                let (count, width) = (ops[i + 2], ops[i + 3]);
                if width > 64 {
                    return fail(
                        BcErrorKind::Unsupported("attribute-range-list-wide"),
                        at,
                        "a constant range list wider than 64 bits",
                    );
                }
                let Some(len) = count.checked_mul(2).and_then(|n| usize::try_from(n).ok()) else {
                    return short();
                };
                need(3 + len)?;
                out.push(Attr::Keyword { kind: ops[i + 1], value: None, ty: None });
                i += 4 + len;
            }
            _ => {
                return fail(
                    BcErrorKind::Unsupported("attribute-encoding"),
                    at,
                    format!("attribute encoding {tag}"),
                );
            }
        }
    }
    Ok(())
}

/// The bytes of a record's characters.
pub(crate) fn chars(ops: &[u64], at: u64) -> BcResult<Vec<u8>> {
    ops.iter()
        .map(|&c| match u8::try_from(c) {
            Ok(byte) => Ok(byte),
            Err(_) => fail(BcErrorKind::Invalid, at, "a character over 255"),
        })
        .collect()
}

/// The keyword of attribute kind `kind`.
pub(crate) fn kind_name(kind: u64, at: u64) -> BcResult<&'static str> {
    match usize::try_from(kind).ok().and_then(|k| KIND_NAMES.get(k)) {
        Some(&name) if !name.is_empty() => Ok(name),
        _ => fail(BcErrorKind::Unsupported("attribute-kind"), at, format!("attribute kind {kind}")),
    }
}

/// A function-attribute group as `hod_ll` builds one (`define_groups`):
/// the interned attributes, the `align`, and the names noted.
#[derive(Clone, Debug)]
pub(crate) struct FnGroup {
    pub attrs: FnAttrsId,
    pub align: Option<u64>,
    pub kept_any: bool,
    pub rejected: Vec<String>,
}

/// Builds a function-index group (`fn_attr` with `in_group`).
pub(crate) fn fn_group<L: Layering>(
    module: &mut hod_ir::Module<L>,
    attrs: &[Attr<'_>],
    at: u64,
) -> BcResult<FnGroup> {
    let mut fa = FnAttrs::new();
    let mut align = None;
    let mut rejected: Vec<String> = Vec::new();
    let mut reject = |name: String| {
        if !rejected.contains(&name) {
            rejected.push(name);
        }
    };
    for attr in attrs {
        match *attr {
            Attr::Str { key, value } => {
                let key = chars(key, at)?;
                let value = chars(value, at)?;
                if ir(at, string_attr(Some(&mut fa), &key, &value))? == AttrUse::Rejected {
                    reject(String::from_utf8_lossy(&key).into_owned());
                }
            }
            Attr::Keyword { kind, value, .. } => {
                let name = kind_name(kind, at)?;
                let Some(row) = attr_info(AttrSyntax::Keyword, name.as_bytes()) else {
                    return fail(
                        BcErrorKind::Invalid,
                        at,
                        format!("attribute '{name}' is not known"),
                    );
                };
                match name {
                    "uwtable" => fa.set_uwtable(match value {
                        None | Some(2) => UwTable::Async,
                        Some(1) => UwTable::Sync,
                        Some(_) => return fail(BcErrorKind::Invalid, at, "an unwind table kind"),
                    }),
                    "alignstack" => ir(at, fa.set_stack_align(int_value(value, at)?))?,
                    "align" => align = Some(int_value(value, at)?),
                    _ => match ir(at, fn_keyword(&mut fa, name.as_bytes(), row.class))? {
                        AttrUse::Kept | AttrUse::Dropped => {}
                        AttrUse::Misplaced => {
                            return fail(
                                BcErrorKind::Invalid,
                                at,
                                format!("attribute '{name}' does not apply to functions"),
                            );
                        }
                        AttrUse::Rejected => reject(name.to_owned()),
                    },
                }
            }
        }
    }
    let kept_any = !fa.is_empty();
    let attrs = ir(at, module.intern_fn_attrs(&fa))?;
    Ok(FnGroup { attrs, align, kept_any, rejected })
}

fn int_value(value: Option<u64>, at: u64) -> BcResult<u64> {
    value.map_or_else(|| fail(BcErrorKind::Invalid, at, "an attribute without its integer"), Ok)
}

/// Return or parameter attributes (`param_attrs`): `ty` resolves a
/// bitcode type id; names noted go to `note`.
pub(crate) fn param_attrs(
    attrs: &[Attr<'_>],
    at: u64,
    ty: &mut dyn FnMut(u64) -> BcResult<hod_ir::TypeId>,
    note: &mut dyn FnMut(String),
) -> BcResult<ParamAttrs> {
    let mut out = ParamAttrs::NONE;
    for attr in attrs {
        let (kind, value, type_id) = match *attr {
            Attr::Str { key, .. } => {
                let key = chars(key, at)?;
                if ir(at, string_attr(None, &key, &[]))? == AttrUse::Rejected {
                    note(attr_construct(&key));
                }
                continue;
            }
            Attr::Keyword { kind, value, ty } => (kind, value, ty),
        };
        let name = kind_name(kind, at)?;
        let Some(row) = attr_info(AttrSyntax::Keyword, name.as_bytes()) else {
            return fail(BcErrorKind::Invalid, at, format!("attribute '{name}' is not known"));
        };
        match value_keyword(name.as_bytes()) {
            ValueKeyword::Flag(flag) => out = ir(at, out.with(flag))?,
            ValueKeyword::Typed(mem) => {
                let Some(type_id) = type_id else {
                    return fail(
                        BcErrorKind::Unsupported("typeless-type-attribute"),
                        at,
                        format!("'{name}' without its type"),
                    );
                };
                let ty = ty(type_id)?;
                match add_typed(out, mem, ty) {
                    Ok((added, construct)) => {
                        if let Some(construct) = construct {
                            note(construct.to_owned());
                        }
                        out = added;
                    }
                    Err(TypedAttrError::TwoTyped) => {
                        return fail(BcErrorKind::Invalid, at, "two typed attributes on one value");
                    }
                    Err(TypedAttrError::Ir(error)) => return ir(at, Err(error)),
                }
            }
            ValueKeyword::Align => out = ir(at, out.with_align(int_value(value, at)?))?,
            ValueKeyword::StackAlign => {
                out = ir(at, out.with_stack_align(int_value(value, at)?))?;
            }
            ValueKeyword::Other => match AttrUse::of_class(row.class) {
                AttrUse::Misplaced => {
                    return fail(
                        BcErrorKind::Invalid,
                        at,
                        format!("attribute '{name}' does not apply to values"),
                    );
                }
                AttrUse::Rejected => note(format!("attr:{name}")),
                AttrUse::Dropped | AttrUse::Kept => {}
            },
        }
    }
    Ok(out)
}
