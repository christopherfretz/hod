//! The attribute table (`spec/ir.md` §5.7, §11): every attribute name in
//! LLVM 19's and LLVM 22's `Attributes.td` is in exactly one class.
//!
//! `tests/data/attributes-llvm{19,22}.txt` are the two name lists, each
//! with its source and the command that derives it. The lists are checked
//! against the table without tools, and against `llvm-as` of the same
//! version when it is installed: every keyword in a list must parse where
//! the list says it may appear, and a keyword the list does not have must
//! not.

mod modlevel;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use hod_ir::{ATTRIBUTES, AttrClass, AttrSyntax, attr_class, attr_info};
use hod_testutil::Scratch;
use modlevel::Assembler;

const LLVM19: &str = include_str!("data/attributes-llvm19.txt");
const LLVM22: &str = include_str!("data/attributes-llvm22.txt");

/// One line of a list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Listed {
    form: &'static str,
    name: &'static str,
    positions: &'static str,
}

impl Listed {
    fn syntax(&self) -> AttrSyntax {
        match self.form {
            "enum" | "int" | "type" | "constantrange" | "constantrangelist" => AttrSyntax::Keyword,
            "strbool" | "complexstr" => AttrSyntax::String,
            other => panic!("unknown attribute form {other}"),
        }
    }
}

fn parse(list: &'static str) -> Vec<Listed> {
    list.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut words = line.split(' ');
            let listed = Listed {
                form: words.next().expect("form"),
                name: words.next().expect("name"),
                positions: words.next().expect("positions"),
            };
            assert_eq!(words.next(), None, "{line}");
            listed
        })
        .collect()
}

/// §5.7 "kept": the keyword attributes that have a representation.
const KEPT_KEYWORDS: [&str; 35] = [
    "align",
    "alignstack",
    "alwaysinline",
    "byref",
    "byval",
    "elementtype",
    "inalloca",
    "inreg",
    "naked",
    "nest",
    "noreturn",
    "nounwind",
    "preallocated",
    "presplitcoroutine",
    "returns_twice",
    "sanitize_address",
    "sanitize_alloc_token",
    "sanitize_hwaddress",
    "sanitize_memory",
    "sanitize_memtag",
    "sanitize_numerical_stability",
    "sanitize_realtime",
    "sanitize_realtime_blocking",
    "sanitize_thread",
    "sanitize_type",
    "signext",
    "sret",
    "ssp",
    "sspreq",
    "sspstrong",
    "swiftasync",
    "swifterror",
    "swiftself",
    "uwtable",
    "zeroext",
];

/// §5.7 "rejected": the keyword attributes LLVM defines that hod neither
/// represents nor may ignore.
const REJECTED_KEYWORDS: [&str; 18] = [
    "convergent",
    "coro_elide_safe",
    "coro_only_destroy_when_complete",
    "fn_ret_thunk_extern",
    "hybrid_patchable",
    "jumptable",
    "nocf_check",
    "nodivergencesource",
    "noduplicate",
    "noimplicitfloat",
    "nonlazybind",
    "noredzone",
    "optdebug",
    "optforfuzzing",
    "safestack",
    "shadowcallstack",
    "speculative_load_hardening",
    "strictfp",
];

/// §5.7 "dropped": the keyword attributes only an optimizer reads.
const DROPPED_KEYWORDS: [&str; 49] = [
    "allocalign",
    "allockind",
    "allocptr",
    "allocsize",
    "builtin",
    "captures",
    "cold",
    "dead_on_return",
    "dead_on_unwind",
    "dereferenceable",
    "dereferenceable_or_null",
    "disable_sanitizer_instrumentation",
    "hot",
    "immarg",
    "initializes",
    "inlinehint",
    "memory",
    "minsize",
    "mustprogress",
    "noalias",
    "nobuiltin",
    "nocallback",
    "nocapture",
    "nocreateundeforpoison",
    "noext",
    "nofpclass",
    "nofree",
    "noinline",
    "nomerge",
    "nonnull",
    "noprofile",
    "norecurse",
    "nosanitize_bounds",
    "nosanitize_coverage",
    "nosync",
    "noundef",
    "null_pointer_is_valid",
    "optnone",
    "optsize",
    "range",
    "readnone",
    "readonly",
    "returned",
    "skipprofile",
    "speculatable",
    "vscale_range",
    "willreturn",
    "writable",
    "writeonly",
];

/// §5.7: the string attributes of `Attributes.td`, by class.
const KEPT_STRINGS: [&str; 1] = ["no-jump-tables"];
const DROPPED_STRINGS: [&str; 6] = [
    "approx-func-fp-math",
    "less-precise-fpmad",
    "no-infs-fp-math",
    "no-nans-fp-math",
    "no-signed-zeros-fp-math",
    "unsafe-fp-math",
];
const REJECTED_STRINGS: [&str; 8] = [
    "allow_direct_access_in_hot_patch_function",
    "denormal-fp-math",
    "denormal-fp-math-f32",
    "loader-replaceable",
    "marked_for_windows_hot_patching",
    "no-inline-line-tables",
    "profile-sample-accurate",
    "use-sample-profile",
];

/// The class the spec's lists give `name`, and how many lists have it.
fn listed_class(syntax: AttrSyntax, name: &str) -> (Option<AttrClass>, usize) {
    let lists: [(&[&str], AttrClass); 3] = match syntax {
        AttrSyntax::Keyword => [
            (&KEPT_KEYWORDS, AttrClass::Kept),
            (&DROPPED_KEYWORDS, AttrClass::Dropped),
            (&REJECTED_KEYWORDS, AttrClass::Rejected),
        ],
        AttrSyntax::String => [
            (&KEPT_STRINGS, AttrClass::Kept),
            (&DROPPED_STRINGS, AttrClass::Dropped),
            (&REJECTED_STRINGS, AttrClass::Rejected),
        ],
    };
    let mut found = None;
    let mut count = 0;
    for (list, class) in lists {
        if list.contains(&name) {
            found = Some(class);
            count += 1;
        }
    }
    (found, count)
}

/// §5.7, §11: every attribute name of LLVM 19 and of LLVM 22 has exactly
/// one row in the table, with the class the spec's lists give it.
#[test]
fn every_llvm_attribute_is_in_exactly_one_class() {
    let llvm19 = parse(LLVM19);
    let llvm22 = parse(LLVM22);
    assert_eq!(llvm19.len(), 104);
    assert_eq!(llvm22.len(), 114);
    let mut union: BTreeMap<(&str, bool), Listed> = BTreeMap::new();
    for listed in llvm19.iter().chain(&llvm22) {
        let key = (listed.name, listed.syntax() == AttrSyntax::Keyword);
        if let Some(previous) = union.insert(key, *listed) {
            // An attribute both versions have is the same kind of thing in
            // both.
            assert_eq!(previous.form, listed.form, "{}", listed.name);
        }
    }
    assert_eq!(union.len(), 117);
    for listed in union.values() {
        let syntax = listed.syntax();
        let rows = ATTRIBUTES
            .iter()
            .filter(|row| row.name == listed.name && row.syntax == syntax && !row.prefix)
            .count();
        assert_eq!(rows, 1, "{} has {rows} rows", listed.name);
        let info = attr_info(syntax, listed.name.as_bytes()).expect("a row");
        assert_eq!(info.name, listed.name);
        assert!(!info.prefix);
        let (class, lists) = listed_class(syntax, listed.name);
        assert_eq!(lists, 1, "{} is in {lists} of the spec's lists", listed.name);
        assert_eq!(Some(info.class), class, "{}", listed.name);
        assert_eq!(attr_class(syntax, listed.name.as_bytes()), info.class);
    }
    // Nothing is on a list that LLVM does not define, and no keyword row
    // is an invention: a keyword LLVM does not have cannot be parsed, so
    // a row for one would be dead.
    let keywords = KEPT_KEYWORDS.iter().chain(&DROPPED_KEYWORDS).chain(&REJECTED_KEYWORDS);
    let strings = KEPT_STRINGS.iter().chain(&DROPPED_STRINGS).chain(&REJECTED_STRINGS);
    assert_eq!(keywords.clone().count() + strings.clone().count(), union.len());
    for name in keywords {
        assert!(union.contains_key(&(*name, true)), "{name} is not an LLVM keyword attribute");
    }
    for name in strings {
        assert!(union.contains_key(&(*name, false)), "{name} is not in Attributes.td");
    }
    for row in ATTRIBUTES.iter().filter(|row| row.syntax == AttrSyntax::Keyword) {
        assert!(union.contains_key(&(row.name, true)), "{} is not an LLVM keyword", row.name);
    }
}

/// §5.7: the string attributes that are not in `Attributes.td` (LLVM
/// defines them where they are read) and the default for everything else.
#[test]
fn string_attributes_outside_the_td_and_unknown_names() {
    let kept = [
        "branch-protection-pauth-lr",
        "branch-target-enforcement",
        "frame-pointer",
        "function-instrument",
        "guarded-control-stack",
        "min-legal-vector-width",
        "no-builtins",
        "patchable-function-entry",
        "patchable-function-prefix",
        "probe-stack",
        "sign-return-address",
        "sign-return-address-key",
        "split-stack",
        "stack-probe-size",
        "stack-protector-buffer-size",
        "target-cpu",
        "target-features",
        "tune-cpu",
        "zero-call-used-regs",
        "no-builtin-memcpy",
        "no-builtin-_Znwm",
    ];
    for name in kept {
        assert_eq!(attr_class(AttrSyntax::String, name.as_bytes()), AttrClass::Kept, "{name}");
    }
    assert_eq!(attr_class(AttrSyntax::String, b"no-trapping-math"), AttrClass::Dropped);
    // The two name spaces are separate, and a name with no row is rejected.
    for (syntax, name) in [
        (AttrSyntax::String, "nounwind"),
        (AttrSyntax::Keyword, "target-cpu"),
        (AttrSyntax::Keyword, "frobnicate"),
        (AttrSyntax::String, "frobnicate"),
        (AttrSyntax::String, "no-builtin-"),
        (AttrSyntax::String, ""),
        (AttrSyntax::Keyword, ""),
        (AttrSyntax::Keyword, "Nounwind"),
    ] {
        assert_eq!(attr_class(syntax, name.as_bytes()), AttrClass::Rejected, "{name}");
        assert!(attr_info(syntax, name.as_bytes()).is_none(), "{name}");
    }
}

/// How `attribute` is written at `position` of a declaration.
fn declaration(index: usize, listed: &Listed) -> String {
    let name = listed.name;
    let text = match name {
        "align" => "align 8".to_string(),
        "alignstack" => "alignstack(8)".to_string(),
        "allockind" => "allockind(\"alloc\")".to_string(),
        "allocsize" => "allocsize(0)".to_string(),
        "captures" => "captures(none)".to_string(),
        "dereferenceable" | "dereferenceable_or_null" => format!("{name}(8)"),
        "memory" => "memory(none)".to_string(),
        "nofpclass" => "nofpclass(nan)".to_string(),
        "uwtable" => "uwtable(sync)".to_string(),
        "vscale_range" => "vscale_range(1,2)".to_string(),
        "range" => "range(i32 0, 10)".to_string(),
        "initializes" => "initializes((0, 4))".to_string(),
        _ if listed.form == "type" => format!("{name}(i32)"),
        _ => {
            assert_eq!(listed.form, "enum", "{name} needs an argument syntax here");
            name.to_string()
        }
    };
    let ty = match name {
        "nofpclass" => "float",
        "range" | "zeroext" | "signext" | "noext" => "i32",
        _ => "ptr",
    };
    if name == "builtin" {
        // A function attribute by the list, and the parser takes it at a
        // call only.
        return format!(
            "declare void @g{index}()\ndefine void @f{index}() {{\n  call void @g{index}() builtin\n  ret void\n}}\n"
        );
    }
    match listed.positions.split(',').next().expect("a position") {
        "fn" => format!("declare void @f{index}() {text}\n"),
        "param" => format!("declare void @f{index}({ty} {text})\n"),
        "ret" => format!("declare {text} {ty} @f{index}()\n"),
        other => panic!("{name}: no position {other}"),
    }
}

/// §11: the checked-in lists are LLVM's. Every keyword of a version's
/// list parses with that version's `llvm-as` at a position the list
/// gives; a keyword only the other version has does not, with the one
/// exception LLVM keeps for old text.
#[test]
fn the_lists_are_what_llvm_as_parses() {
    let lists = [(22, parse(LLVM22), parse(LLVM19)), (19, parse(LLVM19), parse(LLVM22))];
    for (major, list, other) in lists {
        let Some(assembler) = Assembler::new(major) else { continue };
        let scratch = Scratch::new(&format!("attr-table-{major}"));
        let keywords: Vec<&Listed> =
            list.iter().filter(|listed| listed.syntax() == AttrSyntax::Keyword).collect();
        let mut module = String::new();
        for (index, listed) in keywords.iter().enumerate() {
            module.push_str(&declaration(index, listed));
        }
        // Strings are free-form: any parses, on a function.
        for listed in list.iter().filter(|listed| listed.syntax() == AttrSyntax::String) {
            writeln!(module, "declare void @\"s.{0}\"() \"{0}\"=\"x\"", listed.name).unwrap();
        }
        if let Err(error) = assembler.assemble(&scratch, "all", &module, false) {
            panic!("llvm-as {major} refuses the LLVM {major} list:\n{error}");
        }
        // One at a time as well: the batch cannot hide a name that only
        // parses next to another.
        for (index, listed) in keywords.iter().enumerate() {
            let one = declaration(index, listed);
            if let Err(error) = assembler.assemble(&scratch, "one", &one, false) {
                panic!("llvm-as {major} refuses `{}`:\n{error}", one.trim());
            }
        }
        assert!(
            assembler.assemble(&scratch, "bad", "declare void @f() frobnicate\n", false).is_err()
        );
        let foreign = other.iter().filter(|listed| {
            listed.syntax() == AttrSyntax::Keyword
                && !list.iter().any(|mine| mine.name == listed.name)
        });
        let mut accepted = Vec::new();
        let mut count = 0;
        for (index, listed) in foreign.enumerate() {
            count += 1;
            if assembler.assemble(&scratch, "foreign", &declaration(index, listed), false).is_ok() {
                accepted.push(listed.name);
            }
        }
        if major == 22 {
            // LLVM 21 replaced `nocapture` by `captures(none)` and still
            // reads the old spelling.
            assert_eq!(count, 1);
            assert_eq!(accepted, ["nocapture"]);
        } else {
            assert_eq!(count, 10);
            assert_eq!(accepted, [""; 0], "LLVM 19 parses keywords its list does not have");
        }
    }
}
