//! The LLVM side of a batch: for each case, a callee that records its
//! parameters and a caller that passes known arguments, as LLVM IR for
//! `llc` to compile.

use std::fmt::Write as _;

use crate::sigs::{Attr, Plan, Slot};

/// The blobs' symbols: the input, the output of the cases in which hod
/// calls, and the output of those in which LLVM calls.
pub const IN: &str = "hodabi_in";
pub const OUT_A: &str = "hodabi_outa";
pub const OUT_B: &str = "hodabi_outb";

fn at(blob: &str, offset: u64) -> String {
    format!("getelementptr (i8, ptr @{blob}, i64 {offset})")
}

/// Stores `value` of type `ty` at `slot`'s output. An integer is first
/// extended, by sign under `signext` and by zero otherwise: whatever LLVM
/// believes about the bits above the value then shows in the bytes
/// stored.
fn record(text: &mut String, blob: &str, slot: &Slot, ty: &str, value: &str, tag: &str) {
    let dst = at(blob, slot.out);
    match slot.int {
        Some(bits) if bits % 64 != 0 => {
            let op = if slot.ext == Some(true) { "sext" } else { "zext" };
            let wide = recorded_bits(bits, slot.ext);
            let _ = writeln!(text, "  %e{tag} = {op} {ty} {value} to i{wide}");
            let _ = writeln!(text, "  store i{wide} %e{tag}, ptr {dst}, align 1");
        }
        _ => {
            let _ = writeln!(text, "  store {ty} {value}, ptr {dst}, align 1");
        }
    }
}

/// The width LLVM extends a received integer to before storing it: the
/// next multiple of 64 bits, where every belief of 32 or 64 extended bits
/// shows; and 8 bits for a lone `i1`, where the belief that it is a byte
/// of 0 or 1 does.
pub fn recorded_bits(bits: u32, ext: Option<bool>) -> u32 {
    if bits == 1 && ext.is_none() { 8 } else { bits.next_multiple_of(64) }
}

fn header() -> String {
    let mut text = String::from("target triple = \"aarch64-unknown-linux-gnu\"\n\n");
    for blob in [IN, OUT_A, OUT_B] {
        let _ = writeln!(text, "@{blob} = external dso_local global [0 x i8]");
    }
    text.push_str("declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)\n\n");
    text
}

/// The module for `plans`. A case for which `runs(index, hod_calls)` is
/// false gets a stub in that direction.
pub fn module(plans: &[Plan], runs: &dyn Fn(usize, bool) -> bool) -> String {
    let mut text = header();
    for (index, plan) in plans.iter().enumerate() {
        if runs(index, true) {
            callee(&mut text, index, plan);
        } else {
            let _ = writeln!(text, "define void @lf_{index}() {{\n  ret void\n}}\n");
        }
        if runs(index, false) {
            caller(&mut text, index, plan);
        } else {
            let _ = writeln!(text, "define void @lc_{index}() {{\n  ret void\n}}\n");
        }
    }
    text
}

/// A module of the callers `lc_<index>` alone.
pub fn callers(plans: &[Plan]) -> String {
    let mut text = header();
    for (index, plan) in plans.iter().enumerate() {
        caller(&mut text, index, plan);
    }
    text
}

/// `lf_<index>`: records each parameter, returns the return value's image.
fn callee(text: &mut String, index: usize, plan: &Plan) {
    let case = &plan.case;
    let _ = write!(text, "define {}{} @lf_{index}(", case.cc_text(), case.ret_text());
    for (n, param) in case.params.iter().enumerate() {
        let comma = if n == 0 { "" } else { ", " };
        let _ = write!(text, "{comma}{}{} %a{n}", param.ty.text(), param.attr_text());
    }
    text.push_str(") {\n");
    for (n, (param, slot)) in case.params.iter().zip(&plan.args).enumerate() {
        if matches!(param.attr, Attr::ByVal { .. }) {
            if slot.image.size != 0 {
                let _ = writeln!(
                    text,
                    "  call void @llvm.memcpy.p0.p0.i64(ptr align 1 {}, ptr align 1 %a{n}, \
                     i64 {}, i1 false)",
                    at(OUT_A, slot.out),
                    slot.image.size
                );
            }
        } else {
            record(text, OUT_A, slot, &param.ty.text(), &format!("%a{n}"), &n.to_string());
        }
    }
    match (&case.ret, &plan.ret) {
        (Some(ty), Some(slot)) => {
            let ty = ty.text();
            let _ = writeln!(text, "  %r = load {ty}, ptr {}, align 1", at(IN, slot.clean));
            let _ = writeln!(text, "  ret {ty} %r");
        }
        _ => text.push_str("  ret void\n"),
    }
    text.push_str("}\n\n");
}

/// `lc_<index>`: calls `hf_<index>` with each argument's image and records
/// what comes back.
fn caller(text: &mut String, index: usize, plan: &Plan) {
    let case = &plan.case;
    let mut declared = String::new();
    let mut passed = String::new();
    let mut named = String::new();
    let _ = writeln!(text, "define void @lc_{index}() {{");
    for (n, (param, slot)) in case.params.iter().zip(&plan.args).enumerate() {
        let comma = if n == 0 { "" } else { ", " };
        let (ty, attrs) = (param.ty.text(), param.attr_text());
        if case.fixed.is_none_or(|fixed| n < fixed) {
            let _ = write!(declared, "{comma}{ty}{attrs}");
            let _ = write!(named, "{comma}{ty}");
        }
        if matches!(param.attr, Attr::ByVal { .. }) {
            let _ = write!(passed, "{comma}{ty}{attrs} {}", at(IN, slot.clean));
        } else {
            let _ = writeln!(text, "  %v{n} = load {ty}, ptr {}, align 1", at(IN, slot.clean));
            let _ = write!(passed, "{comma}{ty}{attrs} %v{n}");
        }
    }
    let ret = case.ret.as_ref().map_or_else(|| "void".to_string(), crate::ty::Ty::text);
    let mut function_type = String::new();
    if let Some(fixed) = case.fixed {
        let dots = if fixed == 0 { "..." } else { ", ..." };
        declared.push_str(dots);
        let _ = write!(function_type, " ({named}{dots})");
    }
    let result = if case.ret.is_some() { "%r = " } else { "" };
    let _ = writeln!(
        text,
        "  {result}call {}{}{function_type} @hf_{index}({passed})",
        case.cc_text(),
        case.ret_text()
    );
    if let Some(slot) = &plan.ret {
        record(text, OUT_B, slot, &ret, "%r", "r");
    }
    text.push_str("  ret void\n}\n");
    let _ =
        writeln!(text, "declare {}{} @hf_{index}({declared})\n", case.cc_text(), case.ret_text());
}
