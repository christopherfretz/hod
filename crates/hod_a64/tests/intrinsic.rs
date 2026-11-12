//! The intrinsic group (WP-041, `spec/codegen.md` §5.5.1 "Intrinsics"):
//!
//! - the integer intrinsics at every width, against `hod_interp` and
//!   `llc -O0 -global-isel=0 -fast-isel=0` natively
//!   (`common/intrinsic_gen.rs`): a systematic table and random chains;
//! - `memcpy`, `memmove`, `memset` and their `.inline` forms
//!   (`common/intrinsic_mem.rs`): inline and called, constant and
//!   variable lengths, against the interpreter byte for byte with the
//!   buffer at the end of a page, and with many live values across the
//!   call;
//! - what the interpreter does not run, checked natively by a C driver
//!   and against `llc`: the traps, `frameaddress`/`returnaddress`,
//!   `objectsize`, `is.constant`, `prefetch`, `ptrmask`;
//! - the exact words of the short forms; what is refused, by name.

mod common;

#[path = "common/int_gen.rs"]
mod int_gen;
#[path = "common/intrinsic_gen.rs"]
mod intrinsic_gen;

use common::{assert_refused, code_words, compile_ll, ll_header, parse};
use hod_a64::intrinsic;
use hod_cg::Options;
use int_gen::{Report, Rng};
use intrinsic_gen::Gen;

/// Prints what a run measured and panics with the first disagreements.
fn check(name: &str, g: &Gen) {
    let report: Report = intrinsic_gen::run(name, g);
    eprintln!(
        "a64 intrinsic {name}: {} functions, {} cases compared, {} undefined (not compared); \
         {} IR instructions compiled in {:?}; interpreter {:?}, tools {:?}",
        g.suite.funcs.len(),
        report.compared,
        report.undefined,
        report.insts,
        report.compile,
        report.interp,
        report.tools,
    );
    int_gen::assert_agrees(&g.suite, &report);
}

/// Every integer intrinsic at every width, edge and random operands,
/// register and constant operands, both poison flags.
#[test]
fn every_integer_intrinsic_at_every_width_agrees() {
    let g = intrinsic_gen::table(&mut Rng(41));
    check("table", &g);
}

/// Random chains of integer intrinsics and arithmetic.
/// `HOD_INTRINSIC_FUNCTIONS=<n>` sets how many (default 1200).
#[test]
fn random_intrinsic_functions_agree() {
    let count = std::env::var("HOD_INTRINSIC_FUNCTIONS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(1200);
    let g = intrinsic_gen::random(&mut Rng(0x0410), count);
    check("random", &g);
}

/// The words of the only function of `text` (the header added).
fn words(text: &str) -> Vec<u32> {
    let (m, bodies) = parse(&format!("{}{text}", ll_header(22)));
    code_words(&m, &bodies, &Options::default())
}

/// What the group does not lower, by name.
#[test]
fn what_is_not_lowered_is_refused_by_name() {
    assert_refused(&[
        (
            "declare i128 @llvm.ctpop.i128(i128)\n\
             define i128 @f(i128 %a) {\n  %r = call i128 @llvm.ctpop.i128(i128 %a)\n  ret i128 %r\n}\n",
            intrinsic::WIDE,
        ),
        (
            "declare <4 x i32> @llvm.ctpop.v4i32(<4 x i32>)\n\
             define <4 x i32> @f(<4 x i32> %a) {\n  %r = call <4 x i32> @llvm.ctpop.v4i32(<4 x i32> %a)\n  ret <4 x i32> %r\n}\n",
            intrinsic::VECTOR,
        ),
        (
            "declare void @llvm.memcpy.inline.p0.p0.i64(ptr, ptr, i64, i1)\n\
             define void @f(ptr %d, ptr %s, i64 %n) {\n  call void @llvm.memcpy.inline.p0.p0.i64(ptr %d, ptr %s, i64 %n, i1 false)\n  ret void\n}\n",
            intrinsic::INLINE_LENGTH,
        ),
        (
            "declare void @llvm.memset.inline.p0.i64(ptr, i8, i64, i1)\n\
             define void @f(ptr %d, i64 %n) {\n  call void @llvm.memset.inline.p0.i64(ptr %d, i8 1, i64 %n, i1 false)\n  ret void\n}\n",
            intrinsic::INLINE_LENGTH,
        ),
        (
            "declare ptr @llvm.frameaddress.p0(i32)\n\
             define ptr @f() {\n  %r = call ptr @llvm.frameaddress.p0(i32 1000)\n  ret ptr %r\n}\n",
            intrinsic::FRAME_DEPTH,
        ),
        (
            "declare double @llvm.sqrt.f64(double)\n\
             define double @f(double %a) {\n  %r = call double @llvm.sqrt.f64(double %a)\n  ret double %r\n}\n",
            intrinsic::INTRINSIC,
        ),
        (
            "declare i32 @llvm.sadd.sat.i32(i32, i32)\n\
             define i32 @f(i32 %a, i32 %b) {\n  %r = call i32 @llvm.sadd.sat.i32(i32 %a, i32 %b)\n  ret i32 %r\n}\n",
            intrinsic::INTRINSIC,
        ),
    ]);
    // The same intrinsics at 64 bits compile.
    compile_ll(
        "declare i64 @llvm.ctpop.i64(i64)\n\
         define i64 @f(i64 %a) {\n  %r = call i64 @llvm.ctpop.i64(i64 %a)\n  ret i64 %r\n}\n",
    )
    .unwrap();
}

/// The short forms, word for word: `trap` and friends are `llc`'s `brk`s;
/// a 64-bit `ctlz` is one `clz`; a constant `fshl` at 64 bits is one
/// `extr`; `frameaddress(0)` is `mov xD, x29`.
#[test]
fn short_forms_are_llcs() {
    use hod_mc_a64 as mc;
    use hod_mc_a64::{Reg, RegSp, Size};
    let x0 = Reg::X0;
    let traps = words(
        "declare void @llvm.trap()\ndeclare void @llvm.debugtrap()\ndeclare void @llvm.ubsantrap(i8)\n\
         define void @f() {\n  call void @llvm.trap()\n  call void @llvm.debugtrap()\n  \
         call void @llvm.ubsantrap(i8 7)\n  ret void\n}\n",
    );
    let at = |w: u32| traps.iter().position(|&x| x == w).unwrap_or_else(|| panic!("{traps:08x?}"));
    assert!(at(mc::brk(1)) < at(mc::brk(0xf000)) && at(mc::brk(0xf000)) < at(mc::brk(0x5507)));
    let clz = words(
        "declare i64 @llvm.ctlz.i64(i64, i1)\n\
         define i64 @f(i64 %a) {\n  %r = call i64 @llvm.ctlz.i64(i64 %a, i1 false)\n  ret i64 %r\n}\n",
    );
    assert!(clz.contains(&mc::clz(Size::X, x0.zr(), x0.zr())), "{clz:08x?}");
    let extr = words(
        "declare i64 @llvm.fshl.i64(i64, i64, i64)\n\
         define i64 @f(i64 %a, i64 %b) {\n  %r = call i64 @llvm.fshl.i64(i64 %a, i64 %b, i64 13)\n  ret i64 %r\n}\n",
    );
    assert!(extr.contains(&mc::extr(Size::X, x0.zr(), x0.zr(), Reg::X1.zr(), 51)), "{extr:08x?}");
    let fa = words(
        "declare ptr @llvm.frameaddress.p0(i32)\n\
         define ptr @f() {\n  %r = call ptr @llvm.frameaddress.p0(i32 0)\n  ret ptr %r\n}\n",
    );
    let x29 = RegSp::new(29).unwrap();
    assert!(
        fa.iter().any(|&w| (0..31).any(|d| w == mc::mov_sp(Size::X, RegSp::new(d).unwrap(), x29))),
        "{fa:08x?}"
    );
}
