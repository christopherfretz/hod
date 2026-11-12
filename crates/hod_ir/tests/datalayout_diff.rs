//! Data-layout differential tests (`spec/ir.md` §2.1): the parser's grammar
//! and the alignment and size rules, for layout strings other than the two
//! targets', against `opt`.
//!
//! `llc` replaces a module's data layout with the target's, so it cannot
//! observe a custom string. `opt` can: the IR parser gives an `alloca` with
//! no alignment the type's preferred alignment and a `load` its ABI
//! alignment, and `instsimplify` folds `ptrtoint (getelementptr T, null,
//! 1)` to the alloc size. A string LLVM 22 rejects must be an
//! `InvalidDataLayout`, and one it accepts must parse.

mod common;

use std::fmt::Write as _;

use common::{Tools, Ty, edge_types, type_table_for};
use hod_ir::{
    AARCH64_LINUX_DATA_LAYOUT, AARCH64_LINUX_DATA_LAYOUT_LLVM19, DataLayout,
    X86_64_LINUX_DATA_LAYOUT,
};
use hod_testutil::Scratch;

/// Layout strings LLVM 22 accepts.
const ACCEPTED: [&str; 34] = [
    "",
    "e",
    "E",
    AARCH64_LINUX_DATA_LAYOUT,
    AARCH64_LINUX_DATA_LAYOUT_LLVM19,
    X86_64_LINUX_DATA_LAYOUT,
    "e-p:32:32-i64:64-v96:128",
    "e-p:32:32:64-p1:16:16:16:8-i64:32:64-f64:32:64-a:0:32",
    "E-m:o-p:32:32:64:16-p1:64:64-i24:32:64-f80:128-v256:256-a:16:64-n8:16-ni:1:2-S64-P1-A2-G3-Fi8",
    "e-i1:8:16-i8:8:32",
    "e-i1:16:64-i8:8:32",
    "e-i4:32:64",
    "e-i16:16:64",
    "e-i24:32-i128:64",
    "e-i64:64-i128:128-i256:256",
    "e-f16:32-f32:64:128-f64:32-f128:64",
    "e-f80:32",
    "e-f80:128:256",
    "e-v64:32-v128:64-v256:256",
    "e-v24:32-v96:64:128-v16:16",
    "e-a:64",
    "e-a:8:128",
    "e-a0:16",
    "e-p:64:64:64:32",
    "e-p0:16:16-p270:64:64:128",
    "e-pe200:64:64",
    "e-pu200:64:64",
    "e-peu1:64:64",
    "e-s32:32",
    "e-n8:16:32-S128-Fn32",
    "e-ni:1:2:3",
    "e-m:e-P1-A2-G3",
    "e-i8:8:16-i16:16:32-i32:32:64-i64:64:128",
    "e-p:64:64-p1:32:32-p2:16:16-p3:8:8",
];

/// Layout strings LLVM 22 rejects.
const REJECTED: [&str; 59] = [
    "e-",
    "-e",
    "e--m:e",
    "ex",
    "z",
    "x:1",
    ":8",
    "p:64",
    "p:64:64:64:64:64",
    "p:64:64:64:128",
    "p:64:64:64:0",
    "p0:0:64",
    "p:64:0",
    "p:64:24",
    "p16777216:64:64",
    "p-1:64:64",
    "i8:16",
    "i64",
    "i64:0",
    "i64:64:32",
    "i64:64:64:64",
    "i0:8",
    "i16777216:8",
    "i:8",
    "i32:",
    "i32:8:",
    "i32:65536",
    "i32:32:65536",
    "i32:32:0",
    "i+8:8",
    "i 8:8",
    "i0x8:8",
    "i32:12",
    "a",
    "a8:0",
    "a:16:8",
    "a:0:0",
    "n",
    "n0",
    "n8:",
    "S",
    "S0",
    "S24",
    "S65536",
    "F",
    "Fx8",
    "Fn0",
    "Fn",
    "P16777216",
    "P",
    "A1:2",
    "m",
    "m:q",
    "m:ee",
    "me",
    "ni",
    "ni:",
    "ni:0",
    "é",
];

fn probe_types() -> Vec<Ty> {
    let mut types = edge_types();
    let vector = |elem: Ty, len| Ty::Vector(Box::new(elem), len);
    types.extend([
        Ty::Int(4),
        Ty::Int(12),
        Ty::Ptr(2),
        Ty::Ptr(3),
        Ty::Ptr(200),
        vector(Ty::Int(8), 2),
        vector(Ty::Int(8), 3),
        vector(Ty::Int(32), 3),
        vector(Ty::Int(32), 8),
        vector(Ty::Int(64), 4),
        vector(Ty::Ptr(1), 2),
        Ty::Struct(vec![Ty::Int(8), Ty::Ptr(0), Ty::Int(64), Ty::Ptr(1)], false),
        Ty::Struct(vec![Ty::Int(1), Ty::Int(4), Ty::Double, Ty::X86Fp80], false),
        Ty::Array(Box::new(Ty::Struct(vec![Ty::Ptr(1), Ty::Int(16)], false)), 3),
    ]);
    types
}

/// The number after `align ` or `ret i64 ` on the line that defines `name`.
fn number_after<'a>(lines: &mut impl Iterator<Item = &'a str>, start: &str, key: &str) -> u64 {
    let line = lines
        .find(|line| line.trim_start().starts_with(start))
        .unwrap_or_else(|| panic!("no line starting with `{start}`"));
    let at = line.rfind(key).unwrap_or_else(|| panic!("no `{key}` in `{line}`"));
    let digits: String = line[at + key.len()..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or_else(|_| panic!("no number after `{key}` in `{line}`"))
}

/// Compares the type table built from `layout` with LLVM's reading of it.
/// Returns the number of values compared.
fn check_layout(tools: &Tools, scratch: &Scratch, index: usize, layout: &str) -> usize {
    let mut types = type_table_for(tools.major, layout);
    let corpus = probe_types();
    let mut probe = String::from("define void @probe(ptr %p) {\n");
    let mut size_functions = String::new();
    // (type, alloc size, ABI alignment, preferred alignment)
    let mut expected = Vec::new();
    for (at, ty) in corpus.iter().enumerate() {
        let (id, text) = (ty.intern(&mut types), ty.text());
        let _ = writeln!(probe, "  %a{at} = alloca {text}\n  %l{at} = load {text}, ptr %p");
        let _ = writeln!(
            size_functions,
            "define i64 @size{at}() {{\n  %p = getelementptr {text}, ptr null, i32 1\n  \
             %v = ptrtoint ptr %p to i64\n  ret i64 %v\n}}"
        );
        expected.push((text, types.alloc_size_of(id), types.align_of(id), types.pref_align_of(id)));
    }
    probe.push_str("  ret void\n}\n");
    let header = format!("target datalayout = \"{layout}\"\n");
    let name = format!("layout{index}");
    let aligned = tools
        .opt(scratch, &name, &format!("{header}{probe}"), &[])
        .unwrap_or_else(|error| panic!("opt rejected `{layout}`:\n{error}"));
    let sized = tools
        .opt(
            scratch,
            &format!("{name}s"),
            &format!("{header}{size_functions}"),
            &["-passes=instsimplify"],
        )
        .unwrap_or_else(|error| panic!("opt rejected `{layout}`:\n{error}"));
    let (mut aligned, mut sized) = (aligned.lines(), sized.lines());
    let mut failures = Vec::new();
    for (at, (text, size, abi, pref)) in expected.iter().enumerate() {
        let llvm_pref = number_after(&mut aligned, &format!("%a{at} = alloca"), "align ");
        let llvm_abi = number_after(&mut aligned, &format!("%l{at} = load"), "align ");
        number_after(&mut sized, &format!("define i64 @size{at}()"), "@size");
        let llvm_size = number_after(&mut sized, "ret i64", "ret i64 ");
        for (what, hod, llvm) in [
            ("alloc size", size, llvm_size),
            ("ABI alignment", abi, llvm_abi),
            ("preferred alignment", pref, llvm_pref),
        ] {
            if *hod != llvm {
                failures.push(format!("{what} of {text}: hod {hod}, LLVM {llvm}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "LLVM {} with layout `{layout}`: {} mismatches, the first of them:\n{}",
        tools.major,
        failures.len(),
        failures.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
    expected.len() * 3
}

/// §2.1: the alignment and size rules under custom layout strings, as
/// LLVM 22 applies them.
#[test]
fn custom_layouts_match_llvm22() {
    let Some(tools) = common::tools(22) else { return };
    let scratch = Scratch::new("datalayout-22");
    let mut compared = 0;
    for (index, layout) in ACCEPTED.iter().enumerate() {
        compared += check_layout(&tools, &scratch, index, layout);
        // The canonical spelling means the same to LLVM.
        let printed = DataLayout::parse(layout).expect("accepted").to_string();
        tools
            .opt(&scratch, "printed", &format!("target datalayout = \"{printed}\"\n"), &[])
            .unwrap_or_else(|error| panic!("opt rejected `{printed}`:\n{error}"));
    }
    eprintln!(
        "datalayout_diff llvm22: {} layouts, {compared} sizes and alignments",
        ACCEPTED.len()
    );
}

/// §2.1: the two targets' strings as LLVM 19 reads them: LLVM 22's rules
/// plus a default `i1:8:8`.
#[test]
fn target_layouts_match_llvm19() {
    let Some(tools) = common::tools(19) else { return };
    let scratch = Scratch::new("datalayout-19");
    let layouts = [
        AARCH64_LINUX_DATA_LAYOUT_LLVM19,
        X86_64_LINUX_DATA_LAYOUT,
        "e-i4:32:64",
        "e-i1:16:64-i8:8:32",
        "e-p:32:32-i64:64-v96:128",
    ];
    let mut compared = 0;
    for (index, layout) in layouts.iter().enumerate() {
        compared += check_layout(&tools, &scratch, index, layout);
    }
    eprintln!("datalayout_diff llvm19: {} layouts, {compared} sizes and alignments", layouts.len());
}

/// §2.1 `InvalidDataLayout`: the parser accepts exactly the strings
/// LLVM 22 accepts.
#[test]
fn grammar_matches_llvm22() {
    let Some(tools) = common::tools(22) else { return };
    let scratch = Scratch::new("datalayout-grammar");
    let cases = ACCEPTED.iter().map(|text| (*text, true)).chain(REJECTED.map(|text| (text, false)));
    let mut failures = Vec::new();
    for (text, accepted) in cases {
        let llvm =
            tools.opt(&scratch, "grammar", &format!("target datalayout = \"{text}\"\n"), &[]);
        let hod = DataLayout::parse(text);
        if llvm.is_ok() != accepted || hod.is_ok() != accepted {
            failures.push(format!(
                "`{text}`: expected {}, LLVM {}, hod {}",
                if accepted { "accepted" } else { "rejected" },
                llvm.map_or_else(|error| error.trim().to_string(), |_| "accepts".into()),
                hod.map_or_else(|error| error.to_string(), |_| "accepts".into()),
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!(
        "datalayout_diff grammar: {} accepted and {} rejected strings",
        ACCEPTED.len(),
        REJECTED.len()
    );
}
