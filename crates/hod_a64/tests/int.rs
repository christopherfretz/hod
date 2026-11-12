//! The integer group (WP-035, `spec/codegen.md` §5.5.1 "Integers"):
//! binary operations, casts, `icmp`, `select`, `freeze` on `Gpr`-class
//! values at every width, checked against `hod_interp` and `llc -O0
//! -global-isel=0 -fast-isel=0` natively (`common/int_gen.rs`); the
//! compare fusions; what is refused, by name.

mod common;

#[path = "common/int_gen.rs"]
mod int_gen;

use std::time::Instant;

use common::{assert_refused, code_words, compile_ll, ll_header, llc_major, parse};
use hod_a64::{aggr, int};
use hod_cg::Options;
use int_gen::{Report, Rng, Suite, assert_agrees, run};

/// Prints what a suite's run measured.
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn summary(name: &str, suite: &Suite, report: &Report) {
    let per_inst = |bytes: usize| bytes as f64 / report.insts.max(1) as f64;
    eprintln!(
        "a64 int {name}: {} functions, {} cases compared, {} undefined (not compared); \
         {} IR instructions compiled in {:?} ({:.0} ns each, {}); \
         code: hod {:.2} bytes per IR instruction, llc {:.2}; interpreter {:?}, tools {:?}",
        suite.funcs.len(),
        report.compared,
        report.undefined,
        report.insts,
        report.compile,
        report.compile.as_nanos() as f64 / report.insts.max(1) as f64,
        if cfg!(debug_assertions) { "debug" } else { "release" },
        per_inst(report.hod_bytes),
        per_inst(report.llc_bytes),
        report.interp,
        report.tools,
    );
}

fn check(name: &str, suite: &Suite) {
    let report = run(name, suite);
    summary(name, suite, &report);
    assert_agrees(suite, &report);
}

/// Every binary operation at every width, register and constant
/// operands on either side, edge values.
#[test]
fn every_binary_operation_at_every_width_agrees() {
    let suite = int_gen::binary_table(&mut Rng(35));
    check("binary", &suite);
}

/// Every predicate at every width and on pointers, as a value and fused
/// into `select`, `zext` and `sext`; `select` and `freeze`.
#[test]
fn every_compare_and_select_agrees() {
    let suite = int_gen::compare_table(&mut Rng(36));
    check("compare", &suite);
}

/// Every cast between the widths and between integers and pointers.
#[test]
fn every_cast_agrees() {
    let suite = int_gen::cast_table(&mut Rng(37), llc_major() >= 22);
    check("cast", &suite);
}

/// Random straight-line integer functions (with `br`s between blocks).
/// `HOD_INT_FUNCTIONS=<n>` sets how many (default 1500).
#[test]
fn random_integer_functions_agree() {
    let count =
        std::env::var("HOD_INT_FUNCTIONS").ok().and_then(|text| text.parse().ok()).unwrap_or(1500);
    let suite = int_gen::random_functions(&mut Rng(0x0350), count);
    check("random", &suite);
}

/// The words of the only function of `text` (the header added).
fn words(text: &str) -> Vec<u32> {
    let (m, bodies) = parse(&format!("{}{text}", ll_header(22)));
    code_words(&m, &bodies, &Options::default())
}

/// Whether some word of `code` is an instance of `(mask, pattern)`.
fn has(code: &[u32], (mask, pattern): (u32, u32)) -> bool {
    code.iter().any(|w| w & mask == pattern)
}

/// `csel`, `csinc` (`cset`), `csinv` (`csetm`), any size and condition.
const CSEL: (u32, u32) = (0x7fe0_0c00, 0x1a80_0000);
const CSINC: (u32, u32) = (0x7fe0_0c00, 0x1a80_0400);
const CSINV: (u32, u32) = (0x7fe0_0c00, 0x5a80_0000);
/// `subs` (shifted register or immediate) and `adds` immediate: the
/// compares.
const CMP_REG: (u32, u32) = (0x7f20_0000, 0x6b00_0000);
const CMP_IMM: (u32, u32) = (0x7f80_0000, 0x7100_0000);
const CMN_IMM: (u32, u32) = (0x7f80_0000, 0x3100_0000);
/// `tst #1` (`ands` immediate to `wzr`).
const TST: (u32, u32) = (0xff80_001f, 0x7200_001f);

/// §4.6: a compare whose only use is the next `select` is fused into it:
/// `cmp` then `csel`, no `cset`; `zext` and `sext` of a compare are one
/// `cset` or `csetm`; a compare used elsewhere is `cmp` + `cset`.
#[test]
fn compares_fuse_into_select_zext_and_sext() {
    let fused = words(
        "define i64 @f(i64 %a, i64 %b) {\n  %c = icmp slt i64 %a, %b\n  \
         %r = select i1 %c, i64 %a, i64 %b\n  ret i64 %r\n}\n",
    );
    assert!(has(&fused, CMP_REG) && has(&fused, CSEL), "{fused:08x?}");
    assert!(!has(&fused, CSINC) && !has(&fused, TST), "{fused:08x?}");

    let zext = words(
        "define i32 @f(i32 %a) {\n  %c = icmp ult i32 %a, 10\n  %r = zext i1 %c to i32\n  \
         ret i32 %r\n}\n",
    );
    assert!(has(&zext, CMP_IMM) && has(&zext, CSINC), "{zext:08x?}");
    let sext = words(
        "define i64 @f(i64 %a) {\n  %c = icmp eq i64 %a, -5\n  %r = sext i1 %c to i64\n  \
         ret i64 %r\n}\n",
    );
    assert!(has(&sext, CMN_IMM) && has(&sext, CSINV), "{sext:08x?}");

    let apart = words(
        "define i64 @f(i64 %a, i64 %b, i64 %x) {\n  %c = icmp slt i64 %a, %b\n  \
         %y = add i64 %x, 1\n  %r = select i1 %c, i64 %x, i64 %y\n  ret i64 %r\n}\n",
    );
    assert!(has(&apart, CSINC) && has(&apart, TST) && has(&apart, CSEL), "{apart:08x?}");
}

/// The smallest code for the common shapes, word for word: `add` with an
/// immediate in place, `lshr` of an odd width as one `ubfx`, an `i8`
/// compare with a register as `lsl` + `cmp ..., lsl #24`.
#[test]
fn narrow_forms_are_short() {
    use hod_mc_a64 as mc;
    use hod_mc_a64::{Reg, RegZr, Shift, Size};
    let x0 = Reg::X0;
    let add = words("define i32 @f(i32 %a) {\n  %r = add i32 %a, 4096\n  ret i32 %r\n}\n");
    assert!(add.contains(&mc::add_imm(Size::W, x0.sp(), x0.sp(), 4096)), "{add:08x?}");
    let sub = words("define i64 @f(i64 %a) {\n  %r = add i64 %a, -1\n  ret i64 %r\n}\n");
    assert!(sub.contains(&mc::sub_imm(Size::X, x0.sp(), x0.sp(), 1)), "{sub:08x?}");
    let lshr = words("define i17 @f(i17 %a) {\n  %r = lshr i17 %a, 3\n  ret i17 %r\n}\n");
    assert!(lshr.contains(&mc::ubfx(Size::W, x0.zr(), x0.zr(), 3, 14)), "{lshr:08x?}");
    let cmp = words("define i1 @f(i8 %a, i8 %b) {\n  %r = icmp sgt i8 %a, %b\n  ret i1 %r\n}\n");
    assert!(cmp.contains(&mc::lsl_imm(Size::W, x0.zr(), x0.zr(), 24)), "{cmp:08x?}");
    assert!(
        cmp.contains(&mc::cmp_shifted(Size::W, x0.zr(), Reg::X1.zr(), Shift::Lsl, 24)),
        "{cmp:08x?}"
    );
    let not = words("define i64 @f(i64 %a) {\n  %r = xor i64 %a, -1\n  ret i64 %r\n}\n");
    assert!(not.contains(&mc::mvn(Size::X, x0.zr(), x0.zr())), "{not:08x?}");
    let zero = words(
        "define i64 @f(i1 %c, i64 %a) {\n  %r = select i1 %c, i64 %a, i64 0\n  ret i64 %r\n}\n",
    );
    assert!(
        zero.iter().any(|w| (w & 0xffe0_fc00)
            == (mc::csel(Size::X, x0.zr(), x0.zr(), RegZr::ZR, mc::Cond::Ne) & 0xffe0_fc00)),
        "{zero:08x?}"
    );
}

/// What the group does not lower, by name: `addrspacecast` (`classify`
/// rejects its non-zero address spaces anyway), and a `Mem` division wider
/// than 128 bits, which the dispatch sends to `aggr` (WP-040).
#[test]
fn what_is_not_lowered_is_refused_by_name() {
    assert_refused(&[
        (
            "define i64 @f(ptr %p) {\n  %q = addrspacecast ptr %p to ptr addrspace(1)\n  \
             %r = ptrtoint ptr addrspace(1) %q to i64\n  ret i64 %r\n}\n",
            int::ADDRSPACECAST,
        ),
        (
            "define i256 @f(i256 %a, i256 %b) {\n  %r = udiv i256 %a, %b\n  ret i256 %r\n}\n",
            aggr::WIDE_DIVISION,
        ),
    ]);
    // Everything else of the group compiles.
    compile_ll("define i64 @f(i64 %a) {\n  %r = add i64 %a, 1\n  ret i64 %r\n}\n").unwrap();
}

/// §11 (information): nanoseconds per IR instruction of the random
/// functions, with a warm worker, and bytes of code per IR instruction.
#[test]
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn nanoseconds_per_integer_instruction() {
    let suite = int_gen::random_functions(&mut Rng(0x0351), 400);
    let (m, bodies) = parse(&format!("{}{}", ll_header(22), suite.text));
    let insts: usize = bodies.iter().map(hod_ir::Body::inst_count).sum();
    let options = Options::default();
    let mut scratch = hod_cg::Scratch::new();
    let rounds = if cfg!(debug_assertions) { 3 } else { 30 };
    let mut bytes = 0;
    let start = Instant::now();
    for _ in 0..rounds {
        let mut chunk = hod_cg::Chunk::new();
        for body in &bodies {
            hod_cg::compile_function(&hod_a64::A64, &m, body, &options, &mut scratch, &mut chunk)
                .unwrap();
        }
        bytes = chunk.code.len();
    }
    let ns = start.elapsed().as_nanos() as f64 / (rounds * insts) as f64;
    let build = if cfg!(debug_assertions) { "debug" } else { "release" };
    eprintln!(
        "a64 int: {ns:.0} ns per IR instruction ({build}, {insts} instructions, {rounds} rounds); \
         {:.2} bytes of code per IR instruction",
        bytes as f64 / insts as f64
    );
}
