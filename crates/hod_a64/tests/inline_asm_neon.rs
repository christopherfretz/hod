//! NEON in AArch64 inline-assembly templates (`spec/asm.md` §20,
//! WP-226): the operands of the FP/SIMD register classes against `llc`
//! 19 and 22 on a grid of constraint × type × modifier, the census's
//! templates in hod's code as `llvm-mc` writes them, the features of the
//! function, and the refusals. The seeds that run are
//! `tests/data/inline_asm/functions.ll`'s (`tests/inline_asm.rs`).

mod common;

use std::fmt::Write as _;
use std::process::Command;
use std::time::Duration;

use common::{chunk, code_words, compile_ll, ll_header, parse};
use hod_a64::inline_asm;
use hod_asm::a64::{Code, Features, assemble};
use hod_cg::{CgError, Options};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with};

/// The types of the grid.
const TYPES: &[&str] = &[
    "half",
    "bfloat",
    "float",
    "double",
    "fp128",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "ptr",
    "<1 x i8>",
    "<2 x i8>",
    "<8 x i8>",
    "<16 x i8>",
    "<4 x i16>",
    "<2 x half>",
    "<4 x half>",
    "<8 x half>",
    "<2 x float>",
    "<3 x float>",
    "<4 x float>",
    "<1 x i64>",
    "<1 x double>",
    "<2 x double>",
];

/// The types `llc` places in a SIMD&FP register that hod refuses there,
/// by `a64:inline-asm-operand-type`.
const HOD_REFUSES: &[&str] = &["i128", "<2 x i8>", "<2 x half>"];

/// The modifiers of the grid (`None`: no modifier).
const MODIFIERS: &[Option<char>] = &[
    None,
    Some('b'),
    Some('h'),
    Some('s'),
    Some('d'),
    Some('q'),
    Some('w'),
    Some('x'),
    Some('a'),
    Some('z'),
    Some('v'),
    Some('c'),
    Some('n'),
];

/// One input operand of `ty` under `constraint`, printed in `template`
/// (`$OP` stands for the operand with the modifier).
fn cell(major: u32, constraint: &str, ty: &str, modifier: Option<char>, template: &str) -> String {
    let op = modifier.map_or_else(|| "$0".to_string(), |m| format!("${{0:{m}}}"));
    let template = template.replace("$OP", &op);
    format!(
        "{}define void @f({ty} %a) {{\n  call void asm sideeffect \"{template}\", \"{constraint}\"({ty} %a)\n  ret void\n}}\n",
        ll_header(major)
    )
}

/// What `llc-<major>` prints for the operand: the text, or `None` for an
/// error. `Err` when that `llc` is not installed.
fn llc_prints(dir: &Scratch, major: u32, ll: &str) -> Result<Option<String>, ()> {
    let llc = std::path::PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if !llc.is_file() {
        return Err(());
    }
    let input = dir.write(&format!("grid-{major}.ll"), ll);
    let mut command = Command::new(llc);
    command.args(["-O0", "-filetype=asm", "-o", "-"]).arg(&input);
    let out = run_with(&mut command, &[], Duration::from_mins(1));
    if !out.success() {
        return Ok(None);
    }
    let text = out.stdout_text();
    Ok(text
        .split_once("OP=")
        .and_then(|(_, rest)| rest.split_once('='))
        .map(|(printed, _)| printed.to_string()))
}

/// The word of `stmt` as hod's assembler writes it (`llvm_mc.rs` holds
/// these forms to `llvm-mc`).
fn word(stmt: &str) -> u32 {
    let mut code = Code::default();
    assemble(stmt, Features::ALL, &mut code, &mut String::new()).expect(stmt);
    code.words[0]
}

/// The register fields of the probe templates: `Rt` of `str`, `Rd`, `Rn`,
/// `Rm` of `fadd`.
const FIELDS: u32 = 0x001f_03ff;

/// `spec/asm.md` §20: for each constraint, type and modifier, hod prints
/// the operand as `llc` 19 and 22 print it (the template's word is the
/// one of `llc`'s text, the register number aside), or refuses by name
/// where `llc` fails or prints what is no operand (`[s0]`, `z0`).
#[test]
fn operand_grid_is_llc_s() {
    let dir = Scratch::new("a64-inline-asm-grid");
    let mut report = String::new();
    let (mut cells, mut equal, mut refused_too, mut refused_by_hod) = (0, 0, 0, 0);
    let mut cases = Vec::new();
    for ty in TYPES {
        for &m in MODIFIERS {
            cases.push(("w", *ty, m));
        }
        cases.push(("x", *ty, None));
        cases.push(("y", *ty, None));
    }
    for (constraint, ty, m) in cases {
        let mut theirs = None;
        for major in LLVM_MAJORS {
            let Ok(printed) =
                llc_prints(&dir, major, &cell(major, constraint, ty, m, "// OP=$OP="))
            else {
                continue;
            };
            if let Some(seen) = &theirs {
                assert_eq!(seen, &printed, "{constraint} {ty} {m:?}: the majors differ");
            }
            theirs = Some(printed);
        }
        let Some(theirs) = theirs else { return };
        cells += 1;
        // The probe: a form that takes the printed operand.
        let view = theirs.as_deref().and_then(|p| {
            let (first, digits) = p.split_at(1);
            (digits.parse::<u8>().is_ok() && "bhsdqv".contains(first)).then_some(first)
        });
        let (template, want) = match view {
            Some("v") => ("fadd $OP.4s, $OP.4s, $OP.4s", Some(word("fadd v0.4s, v0.4s, v0.4s"))),
            Some(v) => ("str $OP, [x0]", Some(word(&format!("str {v}0, [x0]")))),
            None => ("str $OP, [x0]", None),
        };
        let (m_, bodies) = parse(&cell(22, constraint, ty, m, template));
        let ours = chunk(&m_, &bodies, &Options::default());
        match (want, ours) {
            (Some(want), Ok(chunk)) => {
                let words = chunk
                    .code
                    .chunks(4)
                    .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                    .collect::<Vec<_>>();
                if words.iter().any(|&w| w & !FIELDS == want & !FIELDS) {
                    equal += 1;
                } else {
                    writeln!(report, "  {constraint} {ty} {m:?}: llc {theirs:?}, not hod's")
                        .unwrap();
                }
            }
            (Some(_), Err(e)) => {
                // hod holds no 128-bit integer, and no vector of 2 or 4
                // bytes, in a SIMD&FP register.
                if HOD_REFUSES.contains(&ty) && e == CgError::unsupported(inline_asm::OPERAND_TYPE)
                {
                    refused_by_hod += 1;
                } else {
                    writeln!(report, "  {constraint} {ty} {m:?}: llc {theirs:?}, hod {e}").unwrap();
                }
            }
            (None, Err(_)) => refused_too += 1,
            (None, Ok(_)) => {
                writeln!(report, "  {constraint} {ty} {m:?}: llc {theirs:?}, hod compiles")
                    .unwrap();
            }
        }
    }
    assert!(report.is_empty(), "the operands differ from llc's:\n{report}");
    println!(
        "operand grid: {cells} cells, {equal} printed as llc prints, {refused_too} refused by hod where llc fails or prints no register, \
         {refused_by_hod} refused by hod alone ({HOD_REFUSES:?})"
    );
}

/// The census's templates, as kiln writes them for the projects, in
/// hod's code as `llvm-mc` writes their text with hod's registers.
#[test]
fn census_templates_appear_in_the_code() {
    let cases: &[(&str, &str, &str, &str, &[u32])] = &[
        // (attributes, signature, template, constraints, words)
        (
            "",
            "<4 x float> @f(<4 x float> %c, <4 x float> %a, <4 x float> %b)",
            "fmla $0.4s, $1.4s, $2.s[0]",
            "=w,w,w,0\"(<4 x float> %a, <4 x float> %b, <4 x float> %c",
            // fmla v16.4s, v17.4s, v18.s[0] (v16 the tied output).
            &[0x4f92_1230],
        ),
        (
            "",
            "<4 x i32> @f(<4 x float> %v)",
            "fcvtzs $0.4s, $1.4s",
            "=w,w\"(<4 x float> %v",
            // fcvtzs v16.4s, v17.4s.
            &[0x4ea1_ba30],
        ),
        (
            "",
            "<2 x i64> @f(<2 x i64> %acc, <2 x i32> %a, <2 x i32> %b)",
            "umlal $0.2d, $1.2s, $2.2s",
            "=w,w,w,0\"(<2 x i32> %a, <2 x i32> %b, <2 x i64> %acc",
            // umlal v16.2d, v17.2s, v18.2s.
            &[0x2eb2_8230],
        ),
        (
            "#0",
            "<2 x i64> @f(<2 x i64> %a, <2 x i64> %b)",
            "pmull $0.1q, $1.1d, $2.1d\\0Apmull2 $0.1q, $1.2d, $2.2d",
            "=&w,w,w\"(<2 x i64> %a, <2 x i64> %b",
            // pmull v16.1q, v17.1d, v18.1d; pmull2 v16.1q, v17.2d, v18.2d.
            &[0x0ef2_e230, 0x4ef2_e230],
        ),
    ];
    for &(attrs, sig, template, constraints, want) in cases {
        let (ret, _) = sig.split_once(" @").unwrap();
        let ll = format!(
            "{}define {sig} {attrs} {{\n  %r = call {ret} asm \"{template}\", \"{constraints})\n  ret {ret} %r\n}}\nattributes #0 = {{ \"target-features\"=\"+aes\" }}\n",
            ll_header(22)
        );
        let (m, bodies) = parse(&ll);
        let words = code_words(&m, &bodies, &Options::default());
        // The registers are hod's choice: the fields aside, the words are
        // `llvm-mc`'s.
        let same = |w: &[u32]| w.iter().zip(want).all(|(a, b)| a & !FIELDS == b & !FIELDS);
        assert!(
            words.windows(want.len()).any(same),
            "{template}: {want:08x?} not in the code: {words:08x?}"
        );
    }
}

/// §20: the function's `target-features` decide, as `llc` 19 and 22 do;
/// what is not supported is refused by name.
#[test]
fn neon_refusals_are_named() {
    let one = |attrs: &str, ty: &str, call: &str| {
        format!(
            "define void @f({ty} %a) {attrs} {{\n  {call}\n  ret void\n}}\nattributes #0 = {{ \"target-features\"=\"+aes\" }}\nattributes #1 = {{ \"target-features\"=\"-neon\" }}\n"
        )
    };
    let v4 = "<4 x float>";
    let cases = [
        // `pmull` of `1d` without `aes`; `fullfp16`; no `neon`.
        (
            one(
                "",
                "<2 x i64>",
                "call void asm sideeffect \"pmull v0.1q, $0.1d, $0.1d\", \"w,~{v0}\"(<2 x i64> %a)",
            ),
            "asm:feature",
        ),
        (
            one(
                "",
                v4,
                "call void asm sideeffect \"fcmeq $0.8h, $0.8h, $0.8h\", \"w\"(<4 x float> %a)",
            ),
            "asm:feature",
        ),
        (
            one(
                "#1",
                v4,
                "call void asm sideeffect \"fmla $0.4s, $0.4s, $0.s[1]\", \"w\"(<4 x float> %a)",
            ),
            "asm:feature",
        ),
        // `y` holds nothing but SVE's types; `x` 128-bit values only; `w`
        // no 8-bit value or 96-bit vector (all `llc` errors).
        (
            one("", v4, "call void asm sideeffect \"nop\", \"y\"(<4 x float> %a)"),
            "asm:constraint/y",
        ),
        (
            one("", "double", "call void asm sideeffect \"nop\", \"x\"(double %a)"),
            inline_asm::OPERAND_TYPE,
        ),
        (one("", "i8", "call void asm sideeffect \"nop\", \"w\"(i8 %a)"), inline_asm::OPERAND_TYPE),
        (
            one("", "<3 x float>", "call void asm sideeffect \"nop\", \"w\"(<3 x float> %a)"),
            inline_asm::OPERAND_TYPE,
        ),
        // Modifiers `llc` rejects or prints as no operand.
        (
            one("", v4, "call void asm sideeffect \"nop // ${0:v}\", \"w\"(<4 x float> %a)"),
            "asm:modifier/v",
        ),
        (
            one("", v4, "call void asm sideeffect \"nop // ${0:a}\", \"w\"(<4 x float> %a)"),
            "asm:modifier/a",
        ),
        (
            one("", v4, "call void asm sideeffect \"nop // ${0:z}\", \"w\"(<4 x float> %a)"),
            "asm:modifier/z",
        ),
        // A vector form hod does not take.
        (
            one(
                "",
                v4,
                "call void asm sideeffect \"fmla $0.4s, $0.4s, $0.s[4]\", \"w\"(<4 x float> %a)",
            ),
            "asm:operand/range",
        ),
        (
            one(
                "",
                v4,
                "call void asm sideeffect \"fcvtzs $0.4s, $0.4s, #3\", \"w\"(<4 x float> %a)",
            ),
            "asm:operand/form",
        ),
    ];
    for (ll, reason) in &cases {
        assert_eq!(compile_ll(ll), Err(CgError::unsupported(reason)), "{ll}");
    }
}
