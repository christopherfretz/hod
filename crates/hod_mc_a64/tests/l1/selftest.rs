//! The harness's own negative tests: a batch that disagrees with LLVM is
//! reported, in each of the three ways a batch can disagree. Without these
//! a harness that compared nothing would pass every sweep.

use hod_mc_a64::{Reg, Size, add, sub};

use crate::harness::{Batch, Tools};

const X0: hod_mc_a64::RegZr = Reg::X0.zr();
const X1: hod_mc_a64::RegZr = Reg::X1.zr();
const X2: hod_mc_a64::RegZr = Reg::X2.zr();

fn examine(batch: &Batch) -> Option<(usize, String)> {
    Some(batch.examine(&Tools::find()?))
}

#[test]
fn an_agreeing_batch_is_clean() {
    let mut batch = Batch::default();
    batch.push("add", add(Size::X, X0, X1, X2), None, "add x0, x1, x2".into());
    batch.reject_text("add_imm", "add x0, x1, #4097");
    let Some((mismatches, report)) = examine(&batch) else { return };
    assert_eq!((mismatches, report.as_str()), (0, ""));
}

#[test]
fn a_wrong_word_is_reported() {
    // The right mnemonic, one register off: only the word comparison can
    // see it.
    let mut batch = Batch::default();
    batch.push("add", add(Size::X, X0, X1, X2), None, "add x0, x1, x2".into());
    batch.push("add", add(Size::X, X0, X1, X2), None, "add x0, x1, x3".into());
    let Some((mismatches, report)) = examine(&batch) else { return };
    assert_eq!(mismatches, 1, "{report}");
    assert!(report.contains("`add x0, x1, x3`: hod 0x8b020020, llvm-mc 0x8b030020"), "{report}");
}

#[test]
fn a_wrong_instruction_is_reported_in_both_directions() {
    let mut batch = Batch::default();
    batch.push("add", sub(Size::X, X0, X1, X2), None, "add x0, x1, x2".into());
    let Some((mismatches, report)) = examine(&batch) else { return };
    assert_eq!(mismatches, 2, "{report}");
    assert!(report.contains("hod 0xcb020020, llvm-mc 0x8b020020"), "{report}");
    assert!(report.contains("disassembles as `sub`, expected one of [\"add\"]"), "{report}");
}

#[test]
fn a_wrong_family_member_is_reported() {
    // The text and the word agree, but the case claims another mnemonic.
    let mut batch = Batch::default();
    let word = hod_mc_a64::ldxr(hod_mc_a64::MemSize::B, X0, Reg::X1.sp());
    batch.push("ldxr", word, Some("ldxrh".into()), "ldxrb w0, [x1]".into());
    let Some((mismatches, report)) = examine(&batch) else { return };
    assert_eq!(mismatches, 1, "{report}");
    assert!(report.contains("disassembles as `ldxrb`, expected `ldxrh`"), "{report}");
}

#[test]
fn text_llvm_refuses_is_reported_with_its_line() {
    let mut batch = Batch::default();
    batch.push("add", add(Size::X, X0, X1, X2), None, "add x0, x1, x2".into());
    batch.push("add", add(Size::X, X0, X1, X2), None, "add x0, x1, x99".into());
    batch.push("add", add(Size::X, X0, X1, X2), None, "add x0, x1, x2".into());
    let Some((mismatches, report)) = examine(&batch) else { return };
    assert_eq!(mismatches, 1, "{report}");
    assert!(report.contains("llvm-mc refuses `add x0, x1, x99`"), "{report}");
}

#[test]
fn a_rejection_llvm_accepts_is_reported() {
    // Mixed with a parse-time and a layout-time error, which LLVM reports
    // in separate runs: the accepted line must survive both.
    let mut batch = Batch::default();
    batch.reject_text("add_imm", "add x0, x1, #4097");
    batch.reject_text("add_imm", "add x0, x1, #4095");
    batch.reject_text("b", "b .+0x8000000");
    let Some((mismatches, report)) = examine(&batch) else { return };
    assert_eq!(mismatches, 1, "{report}");
    assert!(report.contains("hod rejects the operands of `add x0, x1, #4095`"), "{report}");
}
