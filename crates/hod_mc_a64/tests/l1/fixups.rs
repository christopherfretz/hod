//! Fixups, `spec/mc-a64.md` "Fixups": patch and extract are inverses and
//! touch only their field (tool-free); patched words equal what LLVM
//! resolves for a label at the same distance.

use std::fmt::Write as _;

use hod_mc_a64::*;

use crate::branch::{HUGE, bad_displacements, displacements, unit};
use crate::harness::{Batch, Tools};
use crate::text::{Rng, z};

/// The field each kind occupies, stated independently of the crate.
fn expected_mask(kind: Fixup) -> u32 {
    match kind {
        Fixup::Jump26 => (1 << 26) - 1,
        Fixup::Branch19 | Fixup::Literal19 => ((1 << 19) - 1) << 5,
        Fixup::Test14 => ((1 << 14) - 1) << 5,
        Fixup::Adr21 | Fixup::Adrp21 => (((1 << 19) - 1) << 5) | (3 << 29),
    }
}

/// (kind, field width in bits).
const WIDTHS: [(Fixup, u32); 6] = [
    (Fixup::Jump26, 26),
    (Fixup::Branch19, 19),
    (Fixup::Test14, 14),
    (Fixup::Adr21, 21),
    (Fixup::Adrp21, 21),
    (Fixup::Literal19, 19),
];

#[test]
fn ranges_are_the_architectures() {
    assert_eq!(Fixup::ALL.len(), WIDTHS.len());
    for (kind, width) in WIDTHS {
        assert!(Fixup::ALL.contains(&kind));
        let unit = unit(kind);
        assert_eq!(kind.min(), -(1i64 << (width - 1)) * unit, "{kind:?}");
        assert_eq!(kind.max(), ((1i64 << (width - 1)) - 1) * unit, "{kind:?}");
        assert_eq!(kind.mask(), expected_mask(kind), "{kind:?}");
    }
    // The headline numbers.
    assert_eq!((Fixup::Jump26.min(), Fixup::Jump26.max()), (-0x800_0000, 0x7ff_fffc));
    assert_eq!((Fixup::Branch19.min(), Fixup::Branch19.max()), (-0x10_0000, 0xf_fffc));
    assert_eq!((Fixup::Test14.min(), Fixup::Test14.max()), (-0x8000, 0x7ffc));
    assert_eq!((Fixup::Adr21.min(), Fixup::Adr21.max()), (-0x10_0000, 0xf_ffff));
    assert_eq!((Fixup::Adrp21.min(), Fixup::Adrp21.max()), (-0x1_0000_0000, 0xffff_f000));
}

#[test]
fn patch_and_extract_are_inverses() {
    let mut rng = Rng::new(0xf1);
    for kind in Fixup::ALL {
        let mask = expected_mask(kind);
        for disp in displacements(kind, 4000) {
            assert!(kind.fits(disp), "{kind:?} {disp}");
            for word in [0, u32::MAX, rng.next() as u32, rng.next() as u32] {
                let patched = kind
                    .patch(word, disp)
                    .unwrap_or_else(|| panic!("{kind:?} {disp} is in range but was rejected"));
                assert_eq!(kind.extract(patched), disp, "{kind:?} {disp} into {word:#010x}");
                assert_eq!(
                    patched & !mask,
                    word & !mask,
                    "{kind:?} changed bits outside its field"
                );
                // Patching again replaces the field; it does not accumulate.
                assert_eq!(kind.patch(patched, 0).map(|w| w & mask), Some(0));
            }
        }
        for disp in bad_displacements(kind).into_iter().chain(HUGE) {
            assert!(!kind.fits(disp), "{kind:?} {disp}");
            assert_eq!(kind.patch(0, disp), None, "{kind:?} {disp}");
        }
    }
}

/// Every field value, exhaustively, for the kinds where that is cheap.
#[test]
fn every_field_value_round_trips() {
    for (kind, width) in WIDTHS {
        if width > 21 {
            continue;
        }
        let unit = unit(kind);
        for field in -(1i64 << (width - 1))..(1i64 << (width - 1)) {
            let disp = field * unit;
            let word = kind.patch(0x1234_5678, disp).expect("in range");
            assert_eq!(kind.extract(word), disp);
        }
    }
}

/// A constructor called with a displacement equals the constructor called
/// with zero, patched: the forward-reference workflow.
#[test]
fn constructors_agree_with_patching() {
    for disp in displacements(Fixup::Jump26, 500) {
        assert_eq!(try_b(disp), Fixup::Jump26.patch(b(0), disp));
        assert_eq!(try_bl(disp), Fixup::Jump26.patch(bl(0), disp));
    }
    for disp in displacements(Fixup::Branch19, 500) {
        assert_eq!(try_b_cond(Cond::Lt, disp), Fixup::Branch19.patch(b_cond(Cond::Lt, 0), disp));
        assert_eq!(
            try_cbz(Size::X, z(3), disp),
            Fixup::Branch19.patch(cbz(Size::X, z(3), 0), disp)
        );
        assert_eq!(
            try_cbnz(Size::W, z(3), disp),
            Fixup::Branch19.patch(cbnz(Size::W, z(3), 0), disp)
        );
    }
    for disp in displacements(Fixup::Test14, 500) {
        assert_eq!(try_tbz(z(3), 40, disp), Fixup::Test14.patch(tbz(z(3), 40, 0), disp));
        assert_eq!(try_tbnz(z(3), 4, disp), Fixup::Test14.patch(tbnz(z(3), 4, 0), disp));
    }
    for disp in displacements(Fixup::Adr21, 500) {
        assert_eq!(try_adr(z(3), disp), Fixup::Adr21.patch(adr(z(3), 0), disp));
    }
    for disp in displacements(Fixup::Adrp21, 500) {
        assert_eq!(try_adrp(z(3), disp), Fixup::Adrp21.patch(adrp(z(3), 0), disp));
    }
    for disp in displacements(Fixup::Literal19, 500) {
        assert_eq!(
            try_ldr_lit(LitLoad::X, z(3), disp),
            Fixup::Literal19.patch(ldr_lit(LitLoad::X, z(3), 0), disp)
        );
    }
}

/// `.+8` / `.-8`: a label at a distance from the instruction itself.
fn label(disp: i64) -> String {
    format!(".{disp:+}")
}

/// Patched words against LLVM resolving a label at the same distance, at
/// the ends of each range and in between; out-of-range and misaligned
/// distances are errors on both sides.
///
/// ADRP is absent: LLVM never resolves an ADRP label itself (the result
/// depends on where the linker places the page), so its field is checked
/// through immediate operands in `branch.rs`.
pub fn build(batch: &mut Batch) {
    macro_rules! through_labels {
        ($($row:literal $kind:ident $mn:literal $zero:expr, $operands:literal;)*) => {$(
            for disp in displacements(Fixup::$kind, 120) {
                batch.push(
                    $row,
                    Fixup::$kind.patch($zero, disp),
                    Some(String::from($mn)),
                    format!(concat!($mn, " ", $operands, "{}"), label(disp)),
                );
            }
            for disp in bad_displacements(Fixup::$kind) {
                batch.reject(
                    $row,
                    Fixup::$kind.patch($zero, disp),
                    &format!("({disp})"),
                    Some(format!(concat!($mn, " ", $operands, "{}"), label(disp))),
                );
            }
        )*};
    }
    through_labels! {
        "patch_jump26" Jump26 "b" b(0), "";
        "patch_jump26" Jump26 "bl" bl(0), "";
        "patch_branch19" Branch19 "b.hi" b_cond(Cond::Hi, 0), "";
        "patch_branch19" Branch19 "cbz" cbz(Size::X, z(9), 0), "x9, ";
        "patch_branch19" Branch19 "cbnz" cbnz(Size::W, z(9), 0), "w9, ";
        "patch_test14" Test14 "tbz" tbz(z(9), 33, 0), "x9, #33, ";
        "patch_test14" Test14 "tbnz" tbnz(z(9), 5, 0), "w9, #5, ";
        "patch_adr21" Adr21 "adr" adr(z(9), 0), "x9, ";
        "patch_literal19" Literal19 "ldr" ldr_lit(LitLoad::X, z(9), 0), "x9, ";
        "patch_literal19" Literal19 "ldrsw" ldr_lit(LitLoad::SwX, z(9), 0), "x9, ";
    }
}

/// One instruction of the layout test: its assembly with `{}` for the
/// label, its word with a zero displacement, and its field.
struct Form {
    asm: &'static str,
    zero: u32,
    kind: Fixup,
}

/// One item of a laid-out function, in address order. The padding puts some
/// labels beyond the reach of the short fields.
enum Item {
    Label(usize),
    Reference,
    Pad(usize),
}

/// The code generator's workflow against real labels: lay out a function
/// with forward and backward references, encode every reference with a
/// zero displacement, patch once positions are known, and compare the
/// whole text with LLVM's.
#[test]
fn a_laid_out_function_matches_llvm() {
    let forms = [
        Form { asm: "b {}", zero: b(0), kind: Fixup::Jump26 },
        Form { asm: "bl {}", zero: bl(0), kind: Fixup::Jump26 },
        Form { asm: "b.ge {}", zero: b_cond(Cond::Ge, 0), kind: Fixup::Branch19 },
        Form { asm: "cbz w4, {}", zero: cbz(Size::W, z(4), 0), kind: Fixup::Branch19 },
        Form { asm: "cbnz x4, {}", zero: cbnz(Size::X, z(4), 0), kind: Fixup::Branch19 },
        Form { asm: "tbz w4, #7, {}", zero: tbz(z(4), 7, 0), kind: Fixup::Test14 },
        Form { asm: "tbnz x4, #63, {}", zero: tbnz(z(4), 63, 0), kind: Fixup::Test14 },
        Form { asm: "adr x4, {}", zero: adr(z(4), 0), kind: Fixup::Adr21 },
        Form { asm: "ldr w4, {}", zero: ldr_lit(LitLoad::W, z(4), 0), kind: Fixup::Literal19 },
    ];
    let mut rng = Rng::new(0x1a7);
    let mut items = Vec::new();
    let mut labels = 0;
    for block in 0..24 {
        for _ in 0..40 {
            if rng.below(5) == 0 {
                items.push(Item::Label(labels));
                labels += 1;
            } else {
                items.push(Item::Reference);
            }
        }
        match block {
            7 => items.push(Item::Pad(0x9000)),
            15 => items.push(Item::Pad(0x10_4000)),
            _ => {}
        }
    }
    // First pass: positions.
    let mut positions = vec![0i64; labels];
    let mut at = 0i64;
    for item in &items {
        match item {
            Item::Label(index) => positions[*index] = at,
            Item::Reference => at += 4,
            Item::Pad(bytes) => at += *bytes as i64,
        }
    }
    // Second pass: choose a target and a form that reaches it, emit and
    // patch.
    let mut source = String::new();
    let mut expected = Vec::new();
    let mut used = [0usize; 9];
    for item in &items {
        match item {
            Item::Label(index) => writeln!(source, "L{index}:").unwrap(),
            Item::Pad(bytes) => {
                writeln!(source, ".space {bytes}").unwrap();
                expected.extend(std::iter::repeat_n(0, bytes / 4));
            }
            Item::Reference => {
                let target = rng.below(labels as u64) as usize;
                let disp = positions[target] - expected.len() as i64 * 4;
                let reaching: Vec<usize> =
                    (0..forms.len()).filter(|&f| forms[f].kind.fits(disp)).collect();
                let form = rng.pick(&reaching);
                used[form] += 1;
                let patched = forms[form].kind.patch(forms[form].zero, disp).expect("fits");
                expected.push(patched);
                source.push_str(&forms[form].asm.replace("{}", &format!("L{target}")));
                source.push('\n');
            }
        }
    }
    assert!(used.iter().all(|&count| count > 10), "every form is exercised: {used:?}");
    let Some(tools) = Tools::find() else { return };
    let words = tools.assemble("layout", &source).unwrap_or_else(|e| panic!("llvm-mc: {e}"));
    assert_eq!(words.len(), expected.len());
    let lines: Vec<&str> = source.lines().filter(|l| !l.ends_with(':')).collect();
    let mut mismatches = Vec::new();
    let mut word = 0;
    for line in lines {
        if let Some(bytes) = line.strip_prefix(".space ") {
            word += bytes.parse::<usize>().unwrap() / 4;
            continue;
        }
        if words[word] != expected[word] {
            mismatches.push(format!(
                "  at {:#x} `{line}`: hod {:#010x}, llvm-mc {:#010x}",
                word * 4,
                expected[word],
                words[word]
            ));
        }
        word += 1;
    }
    assert!(mismatches.is_empty(), "{} mismatches:\n{}", mismatches.len(), mismatches.join("\n"));
    println!("l1 fixups: {word} patched references in a laid-out function equal llvm-mc's");
}
