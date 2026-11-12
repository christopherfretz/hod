//! The field table (`spec/ir.md` §6.9): what becomes of every kind of
//! debug node, every field and every debug record LLVM can write.
//!
//! It is data so that a producer is driven by it instead of repeating
//! it, and so that `tests/debug_survey.rs` can hold everything `clang`
//! emits against it. Three answers:
//!
//! - **mapped**: `hod_ir` has a field for it;
//! - **dropped**: it cannot change the DWARF hod writes, for the reason
//!   given;
//! - **unsupported**: it would change the DWARF and `hod_ir` cannot
//!   represent it; a producer that meets it with a value other than the
//!   field's default records the construct name with
//!   `Module::record_unsupported`.
//!
//! A kind or field that is in no table is unknown, which is unsupported
//! too: the node becomes a [`crate::DiOther`], the field the construct
//! `debug:<kind>.<field>`.

use crate::debug::node::DiKind;

/// What becomes of a kind, a field or a record.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DiMapping {
    /// `hod_ir` represents it.
    Mapped,
    /// Ignored, because it cannot change the DWARF hod writes: the
    /// reason.
    Dropped(&'static str),
    /// Not representable: the construct name for
    /// `Module::record_unsupported`.
    Unsupported(&'static str),
}

const KEY: &str = "a stepping hint (which instructions of a line are statements): hod marks \
                   every line row a statement, as LLVM does without the hint";

macro_rules! notes {
    ($($kind:literal . $field:literal: $what:tt,)*) => {
        &[$(($kind, $field, notes!(@what $kind, $field, $what)),)*]
    };
    (@what $kind:literal, $field:literal, mapped) => { DiMapping::Mapped };
    (@what $kind:literal, $field:literal, unsupported) => {
        DiMapping::Unsupported(concat!("debug:", $kind, ".", $field))
    };
    (@what $kind:literal, $field:literal, (dropped $why:expr)) => { DiMapping::Dropped($why) };
}

/// The fields LLVM 19 to 22 accept that are not a field of the kind's
/// record (those are [`DiKind::fields`]), and the fields of
/// `DILocation`, which is a [`crate::LocId`] and not a node.
pub const DI_FIELD_NOTES: &[(&str, &str, DiMapping)] = notes! {
    "DILocation"."line": mapped,
    "DILocation"."column": mapped,
    "DILocation"."scope": mapped,
    "DILocation"."inlinedAt": mapped,
    "DILocation"."isImplicitCode": (dropped "marks compiler-generated code for coverage tools; \
        LLVM's DWARF emitter does not read it"),
    "DILocation"."atomGroup": (dropped KEY),
    "DILocation"."atomRank": (dropped KEY),
    // The older spelling of `spFlags`: `DiNode::set` folds them in.
    "DISubprogram"."isLocal": mapped,
    "DISubprogram"."isDefinition": mapped,
    "DISubprogram"."isOptimized": mapped,
    "DISubprogram"."virtuality": mapped,
    "DISubprogram"."thisAdjustment": (dropped "read only by LLVM's CodeView emitter"),
    "DISubprogram"."keyInstructions": (dropped KEY),
    "DISubprogram"."annotations": unsupported,
    "DISubprogram"."targetFuncName": unsupported,
    "DICompileUnit"."splitDebugInlining": (dropped "chooses what the skeleton unit of split \
        DWARF repeats; split DWARF itself is debug:DICompileUnit.splitDebugFilename"),
    "DICompileUnit"."debugInfoForProfiling": (dropped "asks for attributes sample profilers \
        use (linkage names on declarations, discriminators); a debugger reads none of them"),
    "DICompileUnit"."rangesBaseAddress": (dropped "an encoding preference for range lists; \
        every encoding describes the same ranges"),
    "DICompileUnit"."splitDebugFilename": unsupported,
    "DICompileUnit"."dwoId": unsupported,
    "DICompileUnit"."macros": unsupported,
    "DICompileUnit"."sourceLanguageName": unsupported,
    "DICompileUnit"."sourceLanguageVersion": unsupported,
    "DIBasicType"."num_extra_inhabitants": unsupported,
    "DIDerivedType"."annotations": unsupported,
    "DIDerivedType"."ptrAuthKey": unsupported,
    "DIDerivedType"."ptrAuthIsAddressDiscriminated": unsupported,
    "DIDerivedType"."ptrAuthExtraDiscriminator": unsupported,
    "DIDerivedType"."ptrAuthIsaPointer": unsupported,
    "DIDerivedType"."ptrAuthAuthenticatesNullValues": unsupported,
    "DICompositeType"."discriminator": unsupported,
    "DICompositeType"."dataLocation": unsupported,
    "DICompositeType"."associated": unsupported,
    "DICompositeType"."allocated": unsupported,
    "DICompositeType"."rank": unsupported,
    "DICompositeType"."annotations": unsupported,
    "DICompositeType"."bitStride": unsupported,
    "DICompositeType"."enumKind": unsupported,
    "DICompositeType"."num_extra_inhabitants": unsupported,
    "DICompositeType"."specification": unsupported,
    "DILocalVariable"."annotations": unsupported,
    "DIGlobalVariable"."annotations": unsupported,
};

/// The node kinds LLVM 19 to 22 accept that are not a [`DiKind`], and
/// the three that are represented without being nodes.
pub const DI_KIND_NOTES: &[(&str, DiMapping)] = &[
    ("DILocation", DiMapping::Mapped),
    ("DIExpression", DiMapping::Mapped),
    ("DIArgList", DiMapping::Mapped),
    (
        "DIAssignID",
        DiMapping::Dropped(
            "ties a #dbg_assign to the stores that carry the same id as instruction metadata, \
             which hod_ir does not keep; an assign record is read as the value record it \
             contains",
        ),
    ),
    ("DIStringType", DiMapping::Unsupported("debug:DIStringType")),
    ("DISubrangeType", DiMapping::Unsupported("debug:DISubrangeType")),
    ("DIFixedPointType", DiMapping::Unsupported("debug:DIFixedPointType")),
    ("DIGenericSubrange", DiMapping::Unsupported("debug:DIGenericSubrange")),
    ("DICommonBlock", DiMapping::Unsupported("debug:DICommonBlock")),
    ("DIModule", DiMapping::Unsupported("debug:DIModule")),
    ("DIObjCProperty", DiMapping::Unsupported("debug:DIObjCProperty")),
    ("DIMacro", DiMapping::Unsupported("debug:DIMacro")),
    ("DIMacroFile", DiMapping::Unsupported("debug:DIMacroFile")),
    ("GenericDINode", DiMapping::Unsupported("debug:GenericDINode")),
];

/// The debug records and the intrinsic calls that are their older
/// spelling: both become the same entries of a body (§6.6).
pub const DI_RECORDS: &[(&str, DiMapping)] = &[
    ("#dbg_declare", DiMapping::Mapped),
    ("#dbg_value", DiMapping::Mapped),
    ("#dbg_assign", DiMapping::Mapped),
    // An assign record's value is one value in `hod_ir` (§6.6).
    ("#dbg_assign.DIArgList", DiMapping::Unsupported("debug:#dbg_assign.DIArgList")),
    ("#dbg_label", DiMapping::Mapped),
    ("#dbg_declare_value", DiMapping::Unsupported("debug:#dbg_declare_value")),
    ("llvm.dbg.declare", DiMapping::Mapped),
    ("llvm.dbg.value", DiMapping::Mapped),
    ("llvm.dbg.assign", DiMapping::Mapped),
    ("llvm.dbg.label", DiMapping::Mapped),
];

/// What becomes of the node kind LLVM writes as `!kind(...)`. `None`
/// for a kind this table does not know: the producer makes it a
/// [`crate::DiOther`], as it does for an unsupported one.
pub fn di_kind(kind: &[u8]) -> Option<DiMapping> {
    if kind.starts_with(b"DI") && DiKind::named(kind).is_some() {
        return Some(DiMapping::Mapped);
    }
    DI_KIND_NOTES.iter().find(|(known, _)| known.as_bytes() == kind).map(|&(_, mapping)| mapping)
}

/// What becomes of `field` of the node kind `kind`. `None` for a field
/// this table does not know: the producer records
/// `debug:<kind>.<field>`.
pub fn di_field(kind: &[u8], field: &[u8]) -> Option<DiMapping> {
    let record = DiKind::named(kind).filter(|_| kind.starts_with(b"DI"));
    if record.is_some_and(|kind| kind.fields().iter().any(|known| known.as_bytes() == field)) {
        return Some(DiMapping::Mapped);
    }
    DI_FIELD_NOTES
        .iter()
        .find(|(known, name, _)| known.as_bytes() == kind && name.as_bytes() == field)
        .map(|&(_, _, mapping)| mapping)
}

/// What becomes of a debug record or debug intrinsic, by the name LLVM
/// writes (`#dbg_value`, `llvm.dbg.value`).
pub fn di_record(name: &[u8]) -> Option<DiMapping> {
    DI_RECORDS.iter().find(|(known, _)| known.as_bytes() == name).map(|&(_, mapping)| mapping)
}

/// Every row of the field table: the kind, the field and what becomes
/// of it. The records' own fields first, kind by kind, then
/// [`DI_FIELD_NOTES`].
pub fn di_field_table() -> impl Iterator<Item = (&'static str, &'static str, DiMapping)> {
    let mapped =
        DiKind::ALL.iter().filter(|kind| kind.llvm_name().starts_with("DI")).flat_map(|kind| {
            kind.fields().iter().map(|field| (kind.llvm_name(), *field, DiMapping::Mapped))
        });
    mapped.chain(DI_FIELD_NOTES.iter().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §6.9: every row is found by its names, no field has two answers,
    /// and an unsupported name is in `classify`'s vocabulary.
    #[test]
    fn table_is_consistent() {
        let rows: Vec<_> = di_field_table().collect();
        assert!(rows.len() > 150, "{}", rows.len());
        for (index, &(kind, field, mapping)) in rows.iter().enumerate() {
            assert_eq!(
                di_field(kind.as_bytes(), field.as_bytes()),
                Some(mapping),
                "{kind}.{field}"
            );
            assert!(
                rows[..index].iter().all(|other| (other.0, other.1) != (kind, field)),
                "{kind}.{field} is listed twice"
            );
            assert!(di_kind(kind.as_bytes()).is_some(), "{kind}");
            match mapping {
                DiMapping::Mapped => {}
                DiMapping::Dropped(why) => assert!(why.len() > 20),
                DiMapping::Unsupported(name) => {
                    assert!(name.starts_with(&format!("debug:{kind}.")), "{name}");
                }
            }
        }
        assert_eq!(di_field(b"DIFile", b"filename"), Some(DiMapping::Mapped));
        assert_eq!(di_field(b"DIFile", b"colour"), None);
        assert_eq!(di_field(b"(other)", b"kind"), None);
        assert_eq!(
            di_field(b"DICompileUnit", b"macros"),
            Some(DiMapping::Unsupported("debug:DICompileUnit.macros"))
        );
        assert!(matches!(di_field(b"DILocation", b"atomRank"), Some(DiMapping::Dropped(_))));
    }

    /// §6.9: kinds and records.
    #[test]
    fn kinds_and_records() {
        assert_eq!(di_kind(b"DISubprogram"), Some(DiMapping::Mapped));
        assert_eq!(di_kind(b"DILocation"), Some(DiMapping::Mapped));
        assert_eq!(di_kind(b"(tuple)"), None);
        assert_eq!(di_kind(b"DIStringType"), Some(DiMapping::Unsupported("debug:DIStringType")));
        assert!(matches!(di_kind(b"DIAssignID"), Some(DiMapping::Dropped(_))));
        assert_eq!(di_kind(b"DIFuture"), None);
        for (kind, mapping) in DI_KIND_NOTES {
            assert!(DiKind::named(kind.as_bytes()).is_none());
            if let DiMapping::Unsupported(name) = mapping {
                assert_eq!(*name, format!("debug:{kind}"));
            }
        }
        assert_eq!(di_record(b"#dbg_declare"), Some(DiMapping::Mapped));
        assert_eq!(di_record(b"llvm.dbg.value"), Some(DiMapping::Mapped));
        assert_eq!(
            di_record(b"#dbg_declare_value"),
            Some(DiMapping::Unsupported("debug:#dbg_declare_value"))
        );
        assert_eq!(di_record(b"#dbg_future"), None);
    }
}
