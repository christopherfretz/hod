//! The i386 join D (`spec/landed/codegen/i386-join-d.md`): the
//! constructs of WP-316 (varargs, atomics, dynamic stack), WP-315
//! (memory intrinsics, aggregates, overflow intrinsics, integers of 33
//! to 63 bits) and WP-314 (exceptions) in the same functions. Programs
//! in `tests/data/i386/join_d/`:
//! - `cmpxchg.ll`: `cmpxchg` at each width (`i8`, `i16`, `i32`, `i64`,
//!   `ptr`), strong and weak, success and failure, its pair read by
//!   `extractvalue` 0 and 1 directly (`atomic::extract`) and after an
//!   aggregate `store`/`load`, `select`, `phi` and `insertvalue`
//!   (`agg`): both must read the value at 0 and the flag at the second
//!   field's offset (`spec/codegen-x86.md` §12.2, §13.2).
//! - `mixed.ll`: `*.with.overflow` + `extractvalue` + `br` with atomic
//!   loops and a `double` live across them in one function; variadic
//!   functions taking and returning aggregates and integers of 33 to 63
//!   bits; a `va_list` kept in a struct and copied as an aggregate.
//! - `eh.ll`: `invoke` of functions returning an aggregate in memory
//!   (first-class and `sret`) and an `{ i64, i1 }` pair, the pad using
//!   the values; landing pads in functions with dynamic `alloca`s and
//!   `stacksave`/`stackrestore`; `memcpy` with an `i64` length in a pad;
//!   an atomic loop inside a `try`.
//! - `fatomic.ll`: `atomicrmw fadd`/`fsub` of `float` and `double`
//!   (`spec/codegen-x86.md` §12.2): rounding, signed zeros, infinities,
//!   a NaN, results used and unused, an x87 value live across the loops.
//!
//! Each is built by `llc` 19 and 22 and by hod (plain and under
//! `Stress::ALL` at the default CPU), at CPU `generic` (no `cmov`) and
//! `i686`, static and PIE, linked by GNU ld, lld and mold, and run under
//! `qemu-i386`: every run must print what `llc`'s prints.

use hod_testutil::Scratch;

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, data, lane};

fn run_all(program: &str, link: &[&str]) {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new(&format!("i386-join-d-{program}"));
    let text = std::fs::read_to_string(data("join_d", program)).unwrap();
    let mut runs = 0;
    for mcpu in [None, Some("i686")] {
        for pic in [false, true] {
            let build = Build { pic, mcpu, link, all_linkers: true, ..STATIC };
            let name = format!("{program}-{}", mcpu.unwrap_or("generic"));
            runs += lane.check(&scratch, &name, &Source::Ll(&text), build);
        }
    }
    assert!(runs > 0, "no run was made");
    eprintln!("i386 join D {program}: {runs} runs equal llc's");
}

#[test]
fn cmpxchg_pairs_read_by_every_lowering() {
    run_all("cmpxchg.ll", &[]);
}

#[test]
fn overflow_atomics_and_varargs_together() {
    run_all("mixed.ll", &[]);
}

#[test]
fn exceptions_with_aggregates_dynamic_stack_and_atomics() {
    run_all("eh.ll", &["-lstdc++"]);
}

#[test]
fn float_atomicrmw_add_and_sub() {
    run_all("fatomic.ll", &[]);
}
