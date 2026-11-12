//! The harness's own negative tests: a batch that disagrees with LLVM is
//! reported, in each of the ways a batch can disagree. Without these a
//! harness that compared nothing would pass every sweep.

use hod_mc_x64::{Reg, Size, add_rr, sub_rr};

use crate::harness::{Batch, Tools};

fn examine(batch: &Batch) -> Vec<(usize, String)> {
    Tools::all().iter().map(|tools| batch.examine(tools)).collect()
}

#[test]
fn an_agreeing_batch_is_clean() {
    let mut batch = Batch::default();
    batch.push("add_rr", add_rr(Size::Q, Reg::Rax, Reg::Rcx), None, "add rax, rcx".into());
    batch.reject_text("add_ri", "add rax, 2147483648");
    for (mismatches, report) in examine(&batch) {
        assert_eq!((mismatches, report.as_str()), (0, ""));
    }
}

#[test]
fn wrong_bytes_are_reported() {
    // The right mnemonic, one register off: only the byte comparison can
    // see it.
    let mut batch = Batch::default();
    batch.push("add_rr", add_rr(Size::Q, Reg::Rax, Reg::Rcx), None, "add rax, rcx".into());
    batch.push("add_rr", add_rr(Size::Q, Reg::Rax, Reg::Rcx), None, "add rax, rdx".into());
    batch.push("add_rr", add_rr(Size::Q, Reg::Rax, Reg::Rcx), None, "add rax, rcx".into());
    for (mismatches, report) in examine(&batch) {
        assert_eq!(mismatches, 1, "{report}");
        assert!(
            report.contains("`add rax, rdx`: hod [48, 01, c8], llvm-mc [48, 01, d0]"),
            "{report}"
        );
    }
}

#[test]
fn a_wrong_length_is_reported_without_misaligning_the_rest() {
    // hod's bytes miss the REX prefix: one byte short, and decoded as
    // another instruction.
    let mut batch = Batch::default();
    batch.push_bytes("add_rr", vec![0x01, 0xc8], None, "add rax, rcx".into());
    batch.push("add_rr", add_rr(Size::D, Reg::Rax, Reg::Rcx), None, "add eax, ecx".into());
    for (mismatches, report) in examine(&batch) {
        assert_eq!(mismatches, 1, "{report}");
        assert!(report.contains("hod [01, c8], llvm-mc [48, 01, c8]"), "{report}");
    }
}

#[test]
fn a_wrong_instruction_is_reported_in_both_directions() {
    let mut batch = Batch::default();
    batch.push("add_rr", sub_rr(Size::Q, Reg::Rax, Reg::Rcx), None, "add rax, rcx".into());
    for (mismatches, report) in examine(&batch) {
        assert_eq!(mismatches, 2, "{report}");
        assert!(report.contains("hod [48, 29, c8], llvm-mc [48, 01, c8]"), "{report}");
        assert!(
            report.contains("disassembles as `sub rax, rcx`: expected one of [\"add\"]"),
            "{report}"
        );
    }
}

#[test]
fn bytes_that_decode_as_two_instructions_are_reported() {
    // `nop; ret` claimed as one instruction: the boundary check sees it.
    let mut batch = Batch::default();
    batch.push_bytes("nop", vec![0x90, 0xc3], None, ".byte 0x90, 0xc3".into());
    for (mismatches, report) in examine(&batch) {
        assert_eq!(mismatches, 1, "{report}");
        assert!(report.contains("the decoded instruction is 1 bytes, not 2"), "{report}");
    }
}

#[test]
fn a_wrong_family_member_is_reported() {
    // The text and the bytes agree, but the case claims another mnemonic.
    let mut batch = Batch::default();
    let inst = hod_mc_x64::setcc_r(hod_mc_x64::Cond::E, Reg::Rax);
    batch.push("setcc_r", inst, Some("setne".into()), "sete al".into());
    for (mismatches, report) in examine(&batch) {
        assert_eq!(mismatches, 1, "{report}");
        assert!(report.contains("disassembles as `sete al`: expected `setne`"), "{report}");
    }
}

#[test]
fn text_llvm_refuses_is_reported_with_its_line() {
    let mut batch = Batch::default();
    let inst = add_rr(Size::Q, Reg::Rax, Reg::Rcx);
    batch.push("add_rr", inst, None, "add rax, rcx".into());
    batch.push("add_rr", inst, None, "addx rax, rcx".into());
    batch.push("add_rr", inst, None, "add rax, rcx".into());
    for (mismatches, report) in examine(&batch) {
        assert_eq!(mismatches, 1, "{report}");
        assert!(report.contains("llvm-mc refuses `addx rax, rcx`"), "{report}");
    }
}

#[test]
fn a_rejection_llvm_accepts_is_reported() {
    let mut batch = Batch::default();
    batch.reject_text("add_ri", "add rax, 2147483648");
    batch.reject_text("add_ri", "add rax, 2147483647");
    batch.reject_text("lea", "lea al, [rax]");
    for (mismatches, report) in examine(&batch) {
        assert_eq!(mismatches, 1, "{report}");
        assert!(report.contains("hod rejects the operands of `add rax, 2147483647`"), "{report}");
    }
}
