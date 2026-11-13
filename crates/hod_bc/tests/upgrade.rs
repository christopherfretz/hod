//! `spec/bc.md` §9.2 and `spec/ir.md` §12 (WP-172): the text and the
//! bitcode of ONE module read to the same module. A bitcode made by a
//! newer `llvm-as` than the text's LLVM is not the same module when
//! LLVM's reader upgrades something on the way: LLVM 22 rewrites LLVM
//! 19's `llvm.aarch64.neon.bfcvtn` and `bfcvtn2` into `fptrunc` and
//! `shufflevector`, so `llvm-as-22` of LLVM 19 text hands `hod_bc` other
//! IR than the text hands `hod_ll`. Both objects are correct for their
//! module; the identity of the two readers is checked with the `llvm-as`
//! of the text's own version.
//!
//! The module uses AArch64 intrinsics, so it cannot be a `tests/ir`
//! seed (every seed builds for both targets); it lives here.

mod common;

/// The smallest LLVM 19 module with the two upgraded intrinsics.
const LLVM19_BFCVTN: &str = r#"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

define <8 x bfloat> @narrow(<4 x float> %lo, <4 x float> %hi) #0 {
  %a = call <8 x bfloat> @llvm.aarch64.neon.bfcvtn(<4 x float> %lo)
  %b = call <8 x bfloat> @llvm.aarch64.neon.bfcvtn2(<8 x bfloat> %a, <4 x float> %hi)
  ret <8 x bfloat> %b
}

declare <8 x bfloat> @llvm.aarch64.neon.bfcvtn(<4 x float>) #1
declare <8 x bfloat> @llvm.aarch64.neon.bfcvtn2(<8 x bfloat>, <4 x float>) #1

attributes #0 = { "target-features"="+bf16,+neon" }
attributes #1 = { nocallback nofree nosync nounwind willreturn memory(none) }
"#;

#[test]
fn text_and_same_version_bitcode_read_equal() {
    let Ok(bitcode) = common::assemble(19, LLVM19_BFCVTN.as_bytes()) else {
        eprintln!("skipped: no llvm-as of LLVM 19");
        return;
    };
    let text = common::read_text_bodies(LLVM19_BFCVTN.as_bytes()).expect("hod_ll reads it");
    let bits = common::read_bitcode_bodies(&bitcode).expect("hod_bc reads it");
    assert_eq!(text, bits, "the text and its LLVM 19 bitcode read to different modules");
    assert!(text.contains("bfcvtn2"), "the intrinsic is read as written:\n{text}");
}

#[test]
fn newer_llvm_as_upgrades_the_module() {
    let Ok(bitcode) = common::assemble(22, LLVM19_BFCVTN.as_bytes()) else {
        eprintln!("skipped: no llvm-as of LLVM 22");
        return;
    };
    let text = common::read_text_bodies(LLVM19_BFCVTN.as_bytes()).expect("hod_ll reads it");
    let bits = common::read_bitcode_bodies(&bitcode).expect("hod_bc reads it");
    // Not a reader disagreement: `llvm-as-22` wrote other IR.
    assert!(!bits.contains("bfcvtn"), "LLVM 22 no longer upgrades bfcvtn:\n{bits}");
    assert_ne!(text, bits);
}
