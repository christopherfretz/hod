//! The support matrix of `classify` (`spec/ir.md` §8.2 to §8.5, §8.8,
//! §8.9): every row has a program that meets it, the two matrices
//! (`shipped`, `everything_implemented`) decide it as the table says,
//! the table is complete, the names are well formed, the first reason is
//! the one the order of §8.8 gives, and the census agrees with repeated
//! classification.

mod classify_support;

use std::collections::BTreeSet;

use classify_support::{
    CC_NUMBERS, CONST_OPS, DOUBLE, I1, I32, NO_STACK, PARAM_FLAGS, PARAM_MEMS, PTR, Program, VOID,
    align, body, define, defined, everything, fx, int, program_for, sym, variable, verdict,
};
use hod_ir::{
    ATTRIBUTES, Arch, AtomicOrdering, AttrClass, CONSTRUCT_FAMILIES, CONSTRUCT_NAMES, CallConv,
    CallDesc, Census, Classifier, ComdatKind, FlagValue, FnAttrs, FnStr, INVALID_RULES, Intrinsic,
    Linkage, Opcode, PicLevel, Rejection, RejectionKind, RmwOp, STRUCTURAL_RULES, SUPPORT_GROUPS,
    SUPPORT_ROWS, Support, SupportStatus, SyncScope, TailKind, TypeClass, TypeClasses, UwTable,
    VariableDesc, Visibility, census,
};

const ARCHES: [Arch; 2] = [Arch::Aarch64, Arch::X86_64];

fn rows_of(group: &str) -> Vec<&'static str> {
    SUPPORT_ROWS
        .iter()
        .filter(|row| row.group() == group)
        .map(|row| row.name.split_once(':').unwrap().1)
        .collect()
}

fn unsupported(result: Result<(), Rejection>, context: &str) -> Rejection {
    let rejection = result.expect_err(context);
    assert_eq!(rejection.kind, RejectionKind::Unsupported, "{context}: {rejection}");
    rejection
}

// ---- §8.2: the matrix decides ----

/// §8.2: for every row and both targets there is a program that meets
/// the row. With everything implemented it is accepted, unless the row
/// is rejected by design, and then the row is the reason; with that one
/// row switched off, the row is the reason. Under the shipped matrix the
/// program is accepted only if it has nothing but shipped constructs.
#[test]
fn every_row_decides_its_own_construct() {
    for arch in ARCHES {
        let all = everything(arch);
        let shipped = Support::shipped(arch);
        for row in &SUPPORT_ROWS {
            let name = row.name;
            let program = program_for(name, arch);
            let context = format!("{name} on {arch:?}");
            if matches!(row.cell(arch).status, SupportStatus::Rejected(_)) {
                let rejection = unsupported(verdict(&all, &program), &context);
                assert_eq!(rejection.construct, name, "{context}");
                assert!(all.clone().with(name).is_none(), "{context}: by design");
            } else {
                assert_eq!(verdict(&all, &program), Ok(()), "{context}");
                let without = all.clone().without(name).unwrap();
                let rejection = unsupported(verdict(&without, &program), &context);
                assert_eq!(rejection.construct, name, "{context}");
            }
            match verdict(shipped, &program) {
                Ok(()) => assert_eq!(shipped.supports(name), Some(true), "{context}"),
                Err(rejection) => {
                    assert_eq!(rejection.kind, RejectionKind::Unsupported, "{context}");
                    let construct = &rejection.construct;
                    assert_eq!(shipped.supports(construct), Some(false), "{context}: {construct}");
                }
            }
        }
    }
}

/// §8.2: the two matrices are the table: `shipped` supports exactly the
/// `supported` cells, `everything_implemented` everything but the
/// `rejected` ones.
#[test]
fn the_matrices_are_the_table() {
    for arch in ARCHES {
        let (all, shipped) = (everything(arch), Support::shipped(arch));
        assert_eq!((all.arch(), shipped.arch()), (arch, arch));
        for row in &SUPPORT_ROWS {
            let cell = row.cell(arch);
            let (is_shipped, is_possible) = match cell.status {
                SupportStatus::Supported => (true, true),
                SupportStatus::NotImplemented(by) => {
                    assert!(!by.is_empty(), "{}: who implements it", row.name);
                    (false, true)
                }
                SupportStatus::Rejected(why) => {
                    assert!(!why.is_empty(), "{}: why", row.name);
                    (false, false)
                }
            };
            assert_eq!(shipped.supports(row.name), Some(is_shipped), "{}", row.name);
            assert_eq!(all.supports(row.name), Some(is_possible), "{}", row.name);
            // A cell says for which classes only when it is supported.
            assert_eq!(cell.classes.is_empty(), !is_shipped, "{}", row.name);
            assert_eq!(shipped.classes(row.name), Some(cell.classes), "{}", row.name);
            let possible = if is_possible { TypeClasses::ALL } else { TypeClasses::NONE };
            assert_eq!(all.classes(row.name), Some(possible), "{}", row.name);
        }
    }
}

/// §8.2: one representative of each group, compiled against the shipped
/// matrix: the first reason of the module pass (empty: it is accepted)
/// and of the body pass (empty: the program has no body or it is
/// accepted), on AArch64 and on x86-64: `(row, module on AArch64, module
/// on x86-64, body on AArch64, body on x86-64)`. A flip of the
/// matrix that changes one of these is a deliberate change of this
/// table. `*` is "not checked here" (no row uses it since WP-066 pinned
/// the AArch64 bodies again once every instruction group was in); `+` is
/// "the body is accepted" where an empty reason would read as "no body"
/// (WP-140: the first x86-64 body accepted whole).
const SHIPPED_REASONS: [(&str, &str, &str, &str, &str); 27] = [
    ("module:pic", "", "", "", ""),
    ("module:frame-pointer", "", "", "", ""),
    ("structor:priority", "", "", "", ""),
    // WP-087: debug modules reach the DWARF writer on AArch64.
    ("debug:module-flags", "", "debug:module-flags", "", ""),
    ("debug:dwarf-version", "debug:dwarf-version", "debug:module-flags", "", ""),
    ("debug:locations", "", "", "", "type:double"),
    ("global:function-declaration", "", "", "", ""),
    // WP-143: x86-64 module-level constructs.
    ("global:variable-definition", "", "", "", ""),
    ("global:ifunc", "", "", "", ""),
    ("linkage:extern_weak", "", "", "", ""),
    ("visibility:hidden", "", "", "", ""),
    ("tls:initial-exec", "", "", "", ""),
    ("comdat:any", "", "", "", ""),
    ("init:symbol", "", "", "", ""),
    ("function:slots", "", "", "", "type:double"),
    // WP-140: `fastcc` is `ccc` on x86-64 too.
    ("cc:fastcc", "", "", "", "+"),
    ("attr:zeroext", "", "", "", "attr:zeroext"),
    ("type:double", "", "", "", "type:double"),
    ("inst:add", "", "", "", "type:double"),
    ("mem:volatile", "", "", "", "type:double"),
    ("atomicrmw:add", "", "", "", "type:double"),
    ("call:indirect", "", "", "", "type:double"),
    ("tail:musttail", "", "", "", "type:double"),
    ("bundle:deopt", "", "", "bundle:deopt", "type:double"),
    ("landingpad:catch", "", "", "", "type:double"),
    ("operand:slot", "", "", "", "type:double"),
    ("intrinsic:llvm.memcpy", "", "", "", "type:double"),
];

#[test]
fn shipped_refuses_with_the_expected_first_reason() {
    let groups: BTreeSet<&str> =
        SHIPPED_REASONS.iter().map(|(row, ..)| row.split_once(':').unwrap().0).collect();
    for (group, _) in SUPPORT_GROUPS {
        // `const-expr:` rows are rejected by design: see the rules test.
        assert!(groups.contains(group) || group == "const-expr", "no representative of {group}");
    }
    for arch in ARCHES {
        let shipped = Classifier::new(Support::shipped(arch)).with_stack_args(&NO_STACK);
        for (row, a64_reason, x64_reason, a64_body, x64_body) in SHIPPED_REASONS {
            let a64 = arch == Arch::Aarch64;
            let module_reason = if a64 { a64_reason } else { x64_reason };
            let body_reason = if a64 { a64_body } else { x64_body };
            let body_reason = if body_reason == "+" { "" } else { body_reason };
            let program = program_for(row, arch);
            let reason = |result: Result<(), Rejection>| {
                result.err().map_or(String::new(), |rejection| rejection.construct.into_owned())
            };
            assert_eq!(reason(shipped.module(&program.m)), module_reason, "{row} on {arch:?}");
            let bodies = program.bodies.first().map(|body| shipped.body(&program.m, body));
            let found = reason(bodies.unwrap_or(Ok(())));
            if body_reason != "*" {
                assert_eq!(found, body_reason, "{row} on {arch:?}");
            }
            assert_eq!(program.bodies.is_empty(), x64_body.is_empty(), "{row}");
        }
    }
}

/// §8.1: the free functions are the shipped matrix of the module's own
/// target, without a `StackArgs`.
#[test]
fn the_free_functions_use_the_shipped_matrix() {
    for arch in ARCHES {
        let shipped = Classifier::new(Support::shipped(arch));
        for (row, ..) in SHIPPED_REASONS {
            let program = program_for(row, arch);
            assert_eq!(hod_ir::classify(&program.m), shipped.module(&program.m), "{row}");
            for body in &program.bodies {
                let expected = shipped.body(&program.m, body);
                assert_eq!(hod_ir::classify_body(&program.m, body), expected, "{row}");
            }
            let mut expected = Census::new();
            shipped.census_module(&program.m, &mut expected);
            for body in &program.bodies {
                shipped.census_body(&program.m, body, &mut expected);
            }
            assert_eq!(census(&program.m, &program.bodies).entries(), expected.entries(), "{row}");
        }
    }
}

// ---- §8.2: `Support` ----

/// §8.2: flipping a row is `with` and `without`; a row that is rejected
/// by design cannot be switched on; an unknown name is `None`.
#[test]
fn support_flips_rows() {
    let shipped = Support::shipped(Arch::Aarch64);
    // A row no package of the first AArch64 wave flips.
    let row = "cc:ghccc";
    assert_eq!(shipped.supports(row), Some(false));
    let flipped = shipped.clone().with(row).unwrap();
    assert_eq!(flipped.supports(row), Some(true));
    assert_eq!(flipped.classes(row), Some(TypeClasses::ALL));
    assert_eq!(flipped.clone().without(row).as_ref(), Some(shipped));
    assert_ne!(&flipped, shipped);

    assert!(shipped.clone().with("attr:naked").is_none(), "rejected by design");
    assert!(shipped.clone().with_classes("attr:naked", TypeClasses::ALL).is_none());
    assert!(shipped.clone().with("type:x86_fp80").is_none(), "not an AArch64 type");
    assert!(Support::shipped(Arch::X86_64).clone().with("type:x86_fp80").is_some());

    for unknown in ["inst:nope", "add", "", "inst:add/nope", "nope/int", "tls:direct-operand"] {
        assert_eq!(shipped.supports(unknown), None, "{unknown:?}");
        assert_eq!(shipped.classes(unknown), None, "{unknown:?}");
        assert!(shipped.clone().with(unknown).is_none(), "{unknown:?}");
        assert!(shipped.clone().without(unknown).is_none(), "{unknown:?}");
    }
    // WP-079: lanes of byte-sized elements, not of the wide integers.
    assert_eq!(shipped.supports("inst:extractelement/int"), Some(true));
    // WP-095: and of `i128` lanes.
    assert_eq!(shipped.supports("inst:extractelement/i128"), Some(true));
    // WP-101: `fp128` lanes, and `insertelement` of both.
    assert_eq!(shipped.supports("inst:extractelement/fp128"), Some(true));
    assert_eq!(shipped.supports("inst:insertelement/i128"), Some(true));
    assert_eq!(shipped.supports("inst:insertelement/fp128"), Some(true));
    // WP-101: the `bfloat` NEON intrinsics.
    assert_eq!(shipped.supports("intrinsic:llvm.aarch64.neon.bfdot"), Some(true));
    assert_eq!(shipped.supports("intrinsic:llvm.aarch64.neon.bfcvtn2"), Some(true));
    // WP-101: a number minus an address, as an operand only.
    assert_eq!(shipped.supports("operand:neg-addr"), Some(true));
    assert_eq!(shipped.supports("init:neg-addr"), Some(false));
    // WP-035: integer arithmetic on integers, not on pointers.
    assert_eq!(shipped.supports("inst:add/int"), Some(true));
    assert_eq!(shipped.supports("inst:add/ptr"), Some(false));
    assert_eq!(shipped.supports("inst:icmp/ptr"), Some(true));
}

/// §8.3: a row can be supported for some classes of type only. A subject
/// type of another class is refused as the pair `row/class`, unless the
/// class itself is unsupported, which is the `type:` row's business.
#[test]
fn a_row_can_be_supported_for_some_classes() {
    let arch = Arch::Aarch64;
    let ints = TypeClasses::of(&[TypeClass::Int]);
    let narrow =
        everything(arch).without("inst:select").unwrap().with_classes("inst:select", ints).unwrap();
    assert_eq!(narrow.classes("inst:select"), Some(ints));
    assert_eq!(narrow.supports("inst:select"), Some(true));
    assert_eq!(narrow.supports("inst:select/int"), Some(true));
    assert_eq!(narrow.supports("inst:select/double"), Some(false));

    let select = |ty| {
        let mut fx = fx(arch);
        let truth = int(&mut fx.m, I1, 1);
        fx.with(|b, _| {
            let value = b.arg(if ty == DOUBLE { 2 } else { 0 }).unwrap();
            b.select(ty, truth, value, value).unwrap();
        })
    };
    assert_eq!(verdict(&narrow, &select(I32)), Ok(()));
    let rejection = unsupported(verdict(&narrow, &select(DOUBLE)), "select double");
    assert_eq!(rejection.construct, "inst:select/double");
    assert!(rejection.symbol.is_some() && rejection.inst.is_some());

    // The pair is a name `with` and `without` take.
    let wider = narrow.clone().with("inst:select/double").unwrap();
    assert_eq!(verdict(&wider, &select(DOUBLE)), Ok(()));
    assert_eq!(wider.without("inst:select/double"), Some(narrow.clone()));

    // An unsupported class is reported as itself, whatever the row has.
    let no_double = narrow.clone().without("type:double").unwrap();
    let rejection = unsupported(verdict(&no_double, &select(DOUBLE)), "no double");
    assert_eq!(rejection.construct, "type:double");

    // Every class of the row gone is the row gone.
    let none = narrow.without("inst:select/int").unwrap();
    assert_eq!(none.supports("inst:select"), Some(false));
    let rejection = unsupported(verdict(&none, &select(I32)), "no select");
    assert_eq!(rejection.construct, "inst:select");
}

// ---- §8.2: completeness ----

/// §8.2: every `Opcode`, `RmwOp`, `Intrinsic`, linkage, COMDAT kind,
/// type class, calling convention and constant operation has a row, in
/// the order of its own list and under LLVM's spelling. Adding one
/// without a row fails here (and, for most, does not compile: the
/// tables' lengths are the lists').
#[test]
fn every_enumerated_construct_has_a_row() {
    let names = |list: &mut dyn Iterator<Item = &'static str>| list.collect::<Vec<_>>();
    assert_eq!(rows_of("inst"), names(&mut Opcode::ALL.iter().map(|op| op.name())));
    assert_eq!(rows_of("atomicrmw"), names(&mut RmwOp::ALL.iter().map(|op| op.name())));
    assert_eq!(rows_of("intrinsic"), names(&mut Intrinsic::ALL.iter().map(|i| i.name())));
    assert_eq!(rows_of("linkage"), names(&mut Linkage::ALL.iter().map(|l| l.keyword())));
    assert_eq!(rows_of("comdat"), names(&mut ComdatKind::ALL.iter().map(|kind| kind.keyword())));
    assert_eq!(rows_of("type"), names(&mut TypeClass::ALL.iter().map(|class| class.name())));
    assert_eq!(rows_of("const-expr"), names(&mut CONST_OPS.iter().map(|op| op.name())));

    let keyword = |number: u32| CallConv::from_llvm(number).ok().and_then(CallConv::keyword);
    assert_eq!(rows_of("cc"), names(&mut CC_NUMBERS.iter().map(|&n| keyword(n).unwrap())));
    for number in 0..1024 {
        assert_eq!(keyword(number).is_some(), CC_NUMBERS.contains(&number), "cc {number}");
    }
    for class in TypeClass::ALL {
        assert_eq!(TypeClass::named(class.name()), Some(class));
    }
}

/// §8.2: the `attr:` rows are the attributes hod keeps (§5.7), neither
/// more nor fewer, in the order of their representations.
#[test]
fn every_kept_attribute_has_a_row() {
    let mut expected: Vec<&str> = Vec::new();
    expected.extend(PARAM_FLAGS.iter().map(|(name, _)| name));
    expected.extend(PARAM_MEMS.iter().map(|(name, _)| name));
    expected.extend(["align", "alignstack", "uwtable"]);
    expected.extend(FnAttrs::FLAG_NAMES.iter().map(|(name, _)| name));
    expected.extend(FnStr::ALL.iter().map(|key| match key {
        FnStr::NoBuiltin => "no-builtin",
        key => key.name(),
    }));
    assert_eq!(rows_of("attr"), expected);

    let rows: BTreeSet<&str> = rows_of("attr").into_iter().collect();
    let kept: BTreeSet<&str> = ATTRIBUTES
        .iter()
        .filter(|info| info.class == AttrClass::Kept)
        .map(|info| if info.prefix { info.name.trim_end_matches('-') } else { info.name })
        .collect();
    assert_eq!(rows, kept);
}

/// §8.4: the groups are the table's sections, each contiguous, in the
/// order of `SUPPORT_GROUPS`.
#[test]
fn the_rows_are_grouped_in_order() {
    let mut met: Vec<&str> = Vec::new();
    for row in &SUPPORT_ROWS {
        if met.last() != Some(&row.group()) {
            assert!(!met.contains(&row.group()), "{} is not contiguous", row.group());
            met.push(row.group());
        }
        assert_eq!(row.name.split_once(':').map(|(group, _)| group), Some(row.group()));
    }
    let groups: Vec<&str> = SUPPORT_GROUPS.iter().map(|(group, _)| *group).collect();
    assert_eq!(met, groups);
    assert!(SUPPORT_GROUPS.iter().all(|(_, what)| !what.is_empty()));
}

// ---- §8.5: the vocabulary ----

fn well_formed(name: &str) -> bool {
    let Some((group, rest)) = name.split_once(':') else { return false };
    let lower = |text: &str, also: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || also.contains(c))
    };
    lower(group, "-") && lower(rest, "-._")
}

/// §8.5: the published names are distinct and of the form
/// `group:name`, lower case. The list is the rows, then the structural
/// rules, then the malformed-input outcomes.
#[test]
fn the_vocabulary_is_well_formed() {
    let mut seen = BTreeSet::new();
    for name in CONSTRUCT_NAMES {
        assert!(well_formed(name), "{name}");
        assert!(seen.insert(name), "{name} is listed twice");
    }
    let listed = SUPPORT_ROWS
        .iter()
        .map(|row| row.name)
        .chain(STRUCTURAL_RULES.iter().map(|rule| rule.name))
        .chain(INVALID_RULES.iter().map(|rule| rule.name));
    assert!(listed.eq(CONSTRUCT_NAMES.iter().copied()));

    for rule in STRUCTURAL_RULES.iter().chain(&INVALID_RULES).chain(&CONSTRUCT_FAMILIES) {
        assert!(!rule.what.is_empty(), "{}", rule.name);
    }
    for rule in INVALID_RULES {
        assert!(rule.name.starts_with("invalid:"), "{}", rule.name);
    }
    // The groups of the rows are not the groups of `hod_abi`'s `abi:`
    // names, which share the namespace (§8.5).
    assert!(SUPPORT_GROUPS.iter().all(|(group, _)| *group != "abi" && *group != "invalid"));
}

// ---- §8.8: the order ----

/// A program with something of nearly every part of the walk.
fn rich(arch: Arch, with_unflippable: bool) -> Program {
    let mut fx = fx(arch);
    let m = &mut fx.m;
    m.props.pic = PicLevel::Big;
    m.props.uwtable = UwTable::Async;
    m.props.dwarf_version = Some(5);
    if with_unflippable {
        m.set_module_flag(1, b"mystery", FlagValue::Int(1)).unwrap();
        m.record_unsupported("attr:jumptable", Some(fx.f.0));
    }
    m.append_module_asm(b".globl x\n");
    let ctor = classify_support::declare(m, "ctor", VOID, &[]).0;
    m.add_ctor(101, ctor, None);
    let pair = m.types.struct_type(&[PTR, PTR], false).unwrap();
    let address = m.consts.addr(fx.g, 0).unwrap();
    let null = m.consts.null(&m.types, PTR).unwrap();
    let init = m.consts.aggregate(&m.types, pair, &[address, null]).unwrap();
    let desc = VariableDesc {
        init: Some(init),
        linkage: Linkage::WeakAny,
        visibility: Visibility::Hidden,
        section: Some(b".mine"),
        ..VariableDesc::new(pair)
    };
    m.add_variable(b"v", &desc).unwrap();
    let g = fx.g;
    let call = CallDesc { tail: TailKind::MustTail, ..fx.call() };
    fx.with(|b, _| {
        let (a, p) = (b.arg(0).unwrap(), b.arg(1).unwrap());
        b.binary(Opcode::Add, I32, a, a).unwrap();
        let slot = b.add_slot(8, 8).unwrap();
        let slot = b.slot(slot).unwrap();
        b.store(slot, p, align(8)).unwrap();
        b.load(I32, sym(g), align(4).with_volatile()).unwrap();
        b.call(VOID, &call).unwrap();
        b.ret(None).unwrap();
    })
}

const RICH_MODULE: [&str; 13] = [
    "module:pic",
    "module:uwtable",
    "debug:module-flags",
    "module:asm-symbols",
    "module:global-ctors",
    "structor:priority",
    "global:function-definition",
    "global:variable-definition",
    "linkage:weak",
    "visibility:hidden",
    "global:section",
    "init:data",
    "init:symbol",
];

/// The rows of `RICH_MODULE` AArch64 supports (WP-032, WP-033, WP-049, WP-075,
/// WP-087).
const A64_SUPPORTED: [&str; 13] = [
    "module:pic",
    "module:uwtable",
    "debug:module-flags",
    "global:function-definition",
    "module:global-ctors",
    "structor:priority",
    "global:variable-definition",
    "linkage:weak",
    "visibility:hidden",
    "global:section",
    "init:data",
    "init:symbol",
    "module:asm-symbols",
];

/// The rows of `RICH_MODULE` x86-64 supports (WP-116, WP-143).
const X64_SUPPORTED: [&str; 12] = [
    "module:pic",
    "module:uwtable",
    "global:function-definition",
    "module:global-ctors",
    "structor:priority",
    "global:variable-definition",
    "linkage:weak",
    "visibility:hidden",
    "global:section",
    "init:data",
    "init:symbol",
    "module:asm-symbols",
];

/// What the shipped matrix finds in the rich module on `arch`.
fn rich_module(arch: Arch) -> Vec<&'static str> {
    let supported: &[&str] = if arch == Arch::Aarch64 { &A64_SUPPORTED } else { &X64_SUPPORTED };
    RICH_MODULE.iter().copied().filter(|row| !supported.contains(row)).collect()
}

const RICH_BODY: [&str; 16] = [
    "cc:ccc",
    "function:parameters",
    "type:int",
    "type:ptr",
    "type:double",
    "function:may-unwind",
    "function:slots",
    "inst:add",
    "inst:store",
    "operand:slot",
    "inst:load",
    "mem:volatile",
    "operand:symbol",
    "inst:call",
    "tail:musttail",
    "inst:ret",
];

/// The rows of `RICH_BODY` AArch64 supports (WP-033, WP-035, WP-036, WP-038, WP-039, WP-049, WP-069).
const A64_SUPPORTED_BODY: [&str; 16] = [
    "cc:ccc",
    "function:parameters",
    "type:int",
    "type:ptr",
    "type:double",
    "operand:symbol",
    "inst:ret",
    "inst:add",
    "function:slots",
    "inst:store",
    "operand:slot",
    "inst:load",
    "mem:volatile",
    "function:may-unwind",
    "inst:call",
    "tail:musttail",
];

/// The rows of `RICH_BODY` x86-64 supports (WP-116, WP-122, WP-123, WP-125).
const X64_SUPPORTED_BODY: [&str; 15] = [
    "cc:ccc",
    "function:parameters",
    "type:int",
    "type:ptr",
    "function:may-unwind",
    "operand:symbol",
    "inst:ret",
    "inst:add",
    // WP-123.
    "function:slots",
    "inst:store",
    "operand:slot",
    "inst:load",
    "mem:volatile",
    // WP-125.
    "inst:call",
    "tail:musttail",
];

/// What the shipped matrix finds in the rich body on `arch`.
fn rich_body(arch: Arch) -> Vec<&'static str> {
    let supported: &[&str] =
        if arch == Arch::Aarch64 { &A64_SUPPORTED_BODY } else { &X64_SUPPORTED_BODY };
    RICH_BODY.iter().copied().filter(|row| !supported.contains(row)).collect()
}

fn census_of(support: &Support, program: &Program) -> Census {
    let classifier = Classifier::new(support).with_stack_args(&NO_STACK);
    let mut census = Census::new();
    classifier.census_module(&program.m, &mut census);
    for body in &program.bodies {
        classifier.census_body(&program.m, body, &mut census);
    }
    census
}

fn names(census: &Census) -> Vec<&str> {
    census.entries().iter().map(|entry| entry.construct.as_str()).collect()
}

/// §8.8: the order findings are made in: the module's properties in the
/// order of §8.8, flags, tables, producer records, the symbols by id
/// and each by its own order; then a body's signature and attributes
/// and its instructions in layout order, each instruction before its
/// sub-rows, its result and its operands.
#[test]
fn findings_come_in_the_specified_order() {
    for arch in ARCHES {
        let program = rich(arch, true);
        let mut expected: Vec<&str> = rich_module(arch);
        // The flags come after the debug flags, which come after the
        // module's own properties; every one before them is supported on
        // AArch64.
        let flags =
            expected.iter().position(|row| *row == "debug:module-flags").map_or(0, |p| p + 1);
        expected.insert(flags, "module-flag:mystery");
        // The function's attributes come before its definition (the first
        // symbol), after the module's own properties.
        let at = expected
            .iter()
            .position(|row| *row == "global:function-definition")
            .or_else(|| expected.iter().position(|row| *row == "global:variable-definition"))
            .unwrap_or(expected.len());
        expected.insert(at, "attr:jumptable");
        let body = rich_body(arch);
        expected.extend(&body);
        let shipped = Support::shipped(arch);
        assert_eq!(names(&census_of(shipped, &program)), expected, "{arch:?}");
        // The first reason of each pass is the first finding of it.
        let classifier = Classifier::new(shipped).with_stack_args(&NO_STACK);
        let module_reason = classifier.module(&program.m).unwrap_err();
        let first =
            if arch == Arch::Aarch64 { "module-flag:mystery" } else { "debug:module-flags" };
        assert_eq!((module_reason.construct.as_ref(), module_reason.symbol), (first, None));
        // AArch64 supports the whole body (WP-069: `tail:musttail`).
        let Some(&first_body) = body.first() else {
            assert_eq!(classifier.body(&program.m, &program.bodies[0]), Ok(()), "{arch:?}");
            continue;
        };
        let body_reason = classifier.body(&program.m, &program.bodies[0]).unwrap_err();
        assert_eq!(body_reason.construct, first_body);
        // Whether it names an instruction depends on the row.
        assert_eq!(body_reason.symbol, Some(program.bodies[0].symbol()));
    }
}

/// §8.8: among the findings of one instruction, a structural rule or a
/// malformed-input outcome comes where the walk meets it, not first:
/// the order is the walk's alone.
#[test]
fn a_rejection_names_where_it_was_found() {
    let arch = Arch::Aarch64;
    let program = rich(arch, false);
    let support = everything(arch).without("mem:volatile").unwrap();
    let rejection = unsupported(verdict(&support, &program), "volatile");
    assert_eq!(rejection.construct, "mem:volatile");
    let body = &program.bodies[0];
    assert_eq!(rejection.symbol, Some(body.symbol()));
    let inst = body.inst(rejection.inst.unwrap());
    assert_eq!(inst.opcode(), Opcode::Load);
    assert_eq!(
        rejection.to_string(),
        format!(
            "unsupported construct mem:volatile at {:?}, instruction {:?}",
            body.symbol(),
            rejection.inst.unwrap()
        )
    );

    // A finding of the module pass at a symbol names the symbol only.
    let support = everything(arch).without("global:section").unwrap();
    let rejection = unsupported(verdict(&support, &program), "section");
    assert_eq!(rejection.construct, "global:section");
    assert_eq!(rejection.symbol, program.m.find_symbol(b"v"));
    assert_eq!(rejection.inst, None);
}

// ---- §8.9: the census ----

/// §8.9: the census is what repeated classification finds: classify,
/// switch the reason on, classify again, until the program is accepted.
#[test]
fn the_census_equals_repeated_classification() {
    for arch in ARCHES {
        let program = rich(arch, false);
        let mut support = Support::shipped(arch).clone();
        let mut found: Vec<String> = Vec::new();
        while let Err(rejection) = verdict(&support, &program) {
            assert!(found.len() < 100, "no progress: {found:?}");
            support = support.with(&rejection.construct).expect("a row that can be switched on");
            found.push(rejection.construct.into_owned());
        }
        let census = census_of(Support::shipped(arch), &program);
        assert_eq!(names(&census), found, "{arch:?}");
        let expected: Vec<&str> = rich_module(arch).into_iter().chain(rich_body(arch)).collect();
        assert_eq!(found, expected, "{arch:?}");
        // And what is accepted has an empty census.
        assert!(census_of(&support, &program).is_empty());
        assert!(census_of(&everything(arch), &program).is_empty());
    }
}

/// §8.9: for every row's program, the first entry of the census is the
/// reason `classify` gives, and the census is empty exactly when the
/// program is accepted.
#[test]
fn the_first_census_entry_is_the_first_reason() {
    for arch in ARCHES {
        for support in [Support::shipped(arch).clone(), everything(arch)] {
            for row in &SUPPORT_ROWS {
                let program = program_for(row.name, arch);
                let census = census_of(&support, &program);
                let first =
                    census.entries().first().map(|entry| (entry.construct.clone(), entry.kind));
                let reason = verdict(&support, &program)
                    .err()
                    .map(|rejection| (rejection.construct.into_owned(), rejection.kind));
                assert_eq!(first, reason, "{} on {arch:?}", row.name);
            }
        }
    }
}

/// §8.9: an entry counts how often its construct was met and at how
/// many symbols; a finding on the module itself has no symbol.
#[test]
fn the_census_counts_occurrences_and_symbols() {
    let arch = Arch::Aarch64;
    let mut m = classify_support::module(arch);
    m.props.pic = PicLevel::Small;
    let one = define(&mut m, "one", VOID, &[I32]);
    let two = define(&mut m, "two", VOID, &[I32]);
    let g = variable(&mut m, "g", I32);
    let seven = m.consts.int(&m.types, I32, 7).unwrap();
    defined(&mut m, "v", seven);
    defined(&mut m, "w", seven);
    let first = body(&m, one, |b, _| {
        let a = b.arg(0).unwrap();
        let sum = b.binary(Opcode::Add, I32, a, a).unwrap();
        b.binary(Opcode::Add, I32, sum, a).unwrap();
        b.va_arg(I32, sym(g)).unwrap();
        b.fence(AtomicOrdering::SeqCst, SyncScope::System).unwrap();
        b.va_arg(I32, sym(g)).unwrap();
        b.store(a, sym(g), align(4)).unwrap();
    });
    let second = body(&m, two, |b, _| {
        let a = b.arg(0).unwrap();
        b.binary(Opcode::Add, I32, a, a).unwrap();
        b.va_arg(I32, sym(g)).unwrap();
    });
    let program = Program { m, bodies: vec![first, second] };
    let census = census_of(Support::shipped(arch), &program);
    let count = |name: &str| {
        let entry = census.get(name).unwrap_or_else(|| panic!("{name} was not counted"));
        assert_eq!((entry.construct.as_str(), entry.kind), (name, RejectionKind::Unsupported));
        (entry.occurrences, entry.symbols)
    };
    // `va_arg` is never compiled (`docs/status/support.md`); `fence` was
    // the example until WP-074.
    assert_eq!(count("inst:va_arg"), (3, 2));
    // Supported since WP-032, WP-033, WP-035, WP-036 and WP-074: not counted.
    for row in [
        "inst:store",
        "global:variable-definition",
        "module:pic",
        "global:function-definition",
        "cc:ccc",
        "type:int",
        "operand:symbol",
        "inst:ret",
        "inst:add",
        "inst:fence",
    ] {
        assert!(census.get(row).is_none(), "{row}");
    }
    assert_eq!(census.len(), census.entries().len());

    // Counting twice counts twice; the symbols stay the same symbols.
    let classifier = Classifier::new(Support::shipped(arch));
    let mut again = census.clone();
    classifier.census_body(&program.m, &program.bodies[1], &mut again);
    assert_eq!(again.get("inst:va_arg").map(|e| (e.occurrences, e.symbols)), Some((4, 2)));
}

// WP-103: inline assembly of instructions and `asm goto` are supported on
// AArch64 (`spec/asm.md`, `spec/codegen.md` §5.5.3); module-level
// assembly an assembler must read is not yet. `RICH_BODY` has no
// inline assembly, so `A64_SUPPORTED_BODY` is unchanged; the rows are
// checked here.
#[test]
fn inline_assembly_rows_of_wp_103() {
    let a64 = Support::shipped(Arch::Aarch64);
    let x64 = Support::shipped(Arch::X86_64);
    for row in ["call:inline-asm", "inst:callbr"] {
        assert_eq!(a64.supports(row), Some(true), "{row}");
        assert_eq!(x64.supports(row), Some(false), "{row}");
        assert!(x64.clone().with(row).is_some(), "{row}: x86-64 may switch it on");
    }
    assert_eq!(a64.supports("module:asm"), Some(false));
}

// WP-128: the x86-64 intrinsics no other group takes, the overflow
// intrinsics on register integers, and empty-template inline assembly.
// `RICH_BODY` has none of them; the rows are checked here.
#[test]
fn intrinsic_rows_of_wp_128() {
    let x64 = Support::shipped(Arch::X86_64);
    for name in [
        "memcpy",
        "memcpy.inline",
        "memmove",
        "memset",
        "memset.inline",
        "frameaddress",
        "returnaddress",
        "trap",
        "debugtrap",
        "ubsantrap",
        "bswap",
        "bitreverse",
        "ctpop",
        "ctlz",
        "cttz",
        "fshl",
        "fshr",
        "abs",
        "smax",
        "smin",
        "umax",
        "umin",
        "scmp",
        "ucmp",
        "sadd.with.overflow",
        "uadd.with.overflow",
        "ssub.with.overflow",
        "usub.with.overflow",
        "smul.with.overflow",
        "umul.with.overflow",
        "sadd.sat",
        "uadd.sat",
        "ssub.sat",
        "usub.sat",
        "eh.typeid.for",
        "prefetch",
        "clear_cache",
        "readcyclecounter",
        "ptrmask",
        "sshl.sat",
        "ushl.sat",
    ] {
        let row = format!("intrinsic:llvm.{name}");
        assert_eq!(x64.supports(&row), Some(true), "{row}");
    }
    assert_eq!(x64.supports("call:inline-asm-empty"), Some(true));
    // On since the target calls `intrinsic::begin` and
    // `intrinsic::dropped` (the instructions `llc` deletes with the call).
    for row in ["intrinsic:llvm.is.constant", "intrinsic:llvm.objectsize"] {
        assert_eq!(x64.supports(row), Some(true), "{row}");
    }
    // Named registers stay with a later package.
    assert_eq!(x64.supports("intrinsic:llvm.read_register"), Some(false));
}
