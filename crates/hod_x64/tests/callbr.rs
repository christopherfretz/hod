//! `callbr` of an empty template on x86-64 (WP-181, `inst:callbr`,
//! `spec/codegen.md` §5.6.23): the inputs are evaluated, a tied output
//! is the input, and the call goes on at its fallthrough with that
//! edge's `phi`s; the indirect destinations are ordinary blocks, entered
//! here only from elsewhere (a branch, an `indirectbr` through their
//! `blockaddress`). The `phi`s of an indirect destination name the
//! `callbr`'s block with a value that must never arrive.
//!
//! Checked against `llc -O0 -fast-isel=0` of LLVM 22 and 19 under the
//! emulator, hod plain and stressed, each linked under every linker; the
//! expected numbers are computed here (the interpreter has no `callbr`).

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use float_gen::{ll_header, parse, pic};
use hod_cg::{CgError, Stress};
use hod_testutil::{Scratch, run_with};
use hod_x64::barrier;

/// The functions: each `callbr` has an empty template.
const FUNCTIONS: &str = r#"
define i32 @cb_void(i32 %x) {
entry:
  callbr void asm sideeffect "", "r,!i,~{dirflag},~{fpsr},~{flags}"(i32 %x)
          to label %ft [label %ind]
ft:
  %a = add i32 %x, 1
  ret i32 %a
ind:
  ret i32 -1
}

define i32 @cb_out(i32 %x) {
entry:
  %y = mul i32 %x, 3
  %r = callbr i32 asm "", "=r,0,!i,~{dirflag},~{fpsr},~{flags}"(i32 %y)
          to label %ft [label %ind]
ind:
  ret i32 -2
ft:
  %s = add i32 %r, 7
  ret i32 %s
}


define i64 @cb_loop(i64 %n) {
entry:
  br label %head
head:
  %i = phi i64 [ 0, %entry ], [ %i1, %body ]
  %acc = phi i64 [ 0, %entry ], [ %acc1, %body ]
  %c = icmp ult i64 %i, %n
  br i1 %c, label %go, label %out
go:
  callbr void asm "", "r,r,!i,!i"(i64 %i, i64 %acc)
          to label %body [label %side, label %out]
side:
  ret i64 -1
out:
  %res = phi i64 [ %acc, %head ], [ 999, %go ]
  ret i64 %res
body:
  %acc1 = add i64 %acc, %i
  %i1 = add i64 %i, 1
  br label %head
}

define i32 @cb_addr(i32 %x) {
entry:
  %odd = and i32 %x, 1
  %c = icmp ne i32 %odd, 0
  br i1 %c, label %jump, label %asm
asm:
  callbr void asm sideeffect "", "r,!i"(i32 %x) to label %ft [label %ind]
ft:
  %d = shl i32 %x, 1
  ret i32 %d
jump:
  indirectbr ptr blockaddress(@cb_addr, %ind), [label %ind]
ind:
  %v = phi i32 [ 100, %asm ], [ %x, %jump ]
  %w = add i32 %v, 1000
  ret i32 %w
}

define i32 @cb_mem(ptr %p, i32 %x) {
entry:
  %r = callbr i32 asm sideeffect "", "=r,0,*m,!i,!i,~{memory}"(i32 %x, ptr elementtype(i32) %p)
          to label %ft [label %ind, label %ft]
ft:
  %l = load i32, ptr %p, align 4
  %s = add i32 %l, %r
  ret i32 %s
ind:
  ret i32 -3
}
"#;

const DRIVER: &str = r#"#include <stdio.h>
int cb_void(int);
int cb_out(int);
long cb_loop(long);
int cb_addr(int);
int cb_mem(int *, int);
int main(void) {
  for (int i = -3; i < 8; i++) {
    int m = 40 + i;
    printf("%d %d %ld %d %d\n", cb_void(i), cb_out(i), cb_loop(i < 0 ? 0 : i), cb_addr(i),
           cb_mem(&m, i));
  }
  return 0;
}
"#;

/// What the driver prints: the fallthrough is always taken.
fn expected() -> String {
    let mut lines = String::new();
    for i in -3i64..8 {
        let n = i.max(0);
        let addr = if i & 1 != 0 { i + 1000 } else { 2 * i };
        let (out, sum, mem) = (3 * i + 7, n * (n - 1) / 2, 40 + 2 * i);
        writeln!(lines, "{} {out} {sum} {addr} {mem}", i + 1).unwrap();
    }
    lines
}

#[test]
fn empty_template_callbr_runs_as_llc_builds_it() {
    let text = format!("{}{FUNCTIONS}", ll_header());
    let (module, bodies) = parse(&text);
    let classifier = hod_ir::Classifier::new(hod_ir::Support::shipped(hod_ir::Arch::X86_64));
    for body in &bodies {
        classifier.body(&module, body).unwrap_or_else(|r| panic!("classify: {}", r.construct));
    }
    let scratch = Scratch::new("x64-callbr");
    let mut objects = Vec::new();
    for (leg, stress) in [("hod-plain", Stress::default()), ("hod-stress", Stress::ALL)] {
        let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{leg}: {e}"));
        objects.push((leg.to_owned(), scratch.write(&format!("{leg}.o"), object)));
    }
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    for major in [22, 19] {
        if let Some(object) = float_gen::llc(&scratch, major, &format!("llc{major}"), &text) {
            objects.push((format!("llc{major}"), object));
        }
    }
    let driver = toolchain.compile_c(&scratch, "driver", DRIVER, &[]);
    let want = expected();
    let mut ran = Vec::new();
    for (leg, object) in &objects {
        for linker in &toolchain.linkers {
            let program = scratch.join(format!("{leg}-{}", linker.name));
            let inputs: [&Path; 2] = [&driver, object];
            toolchain.link_ok(linker, &[], &inputs, &program);
            let outcome =
                run_with(&mut toolchain.runner_command(&program), &[], Duration::from_mins(2));
            assert!(outcome.success(), "{leg}/{}: {}", linker.name, outcome.stderr_text());
            assert_eq!(outcome.stdout_text(), want, "{leg}/{}", linker.name);
            ran.push(format!("{leg}/{}", linker.name));
        }
    }
    eprintln!("x64 callbr: 5 functions x 11 arguments agree over {ran:?}");
}

/// A `callbr` result that a `phi` of a destination takes besides another
/// use (a home value): the shared walk has written that `phi` before the
/// lowering, so it is refused by name.
const RESULT_PHI: &str = r#"
define i32 @cb_phi(i32 %x) {
entry:
  %r = callbr i32 asm "", "=r,0,!i"(i32 %x) to label %ft [label %ind]
ind:
  ret i32 -4
ft:
  %p = phi i32 [ %r, %entry ]
  %q = mul i32 %r, 5
  %s = sub i32 %q, %p
  ret i32 %s
}
"#;

/// A `callbr` whose template assembles to something stays refused, and
/// so does one whose result a destination's `phi` takes.
#[test]
fn callbr_of_instructions_is_refused_by_name() {
    let (m, bodies) = parse(&format!("{}{RESULT_PHI}", ll_header()));
    match hod_x64::compile_to_object(&m, &bodies, &pic(Stress::default())) {
        Err(hod_x64::ModuleError::Function { error, .. }) => {
            assert_eq!(error, CgError::unsupported(barrier::RESULT_PHI));
        }
        other => panic!("not refused: {:?}", other.map(|o| o.len())),
    }
    let text = format!(
        "{}define void @f() {{\nentry:\n  callbr void asm \"jmp ${{0:l}}\", \"!i\"() to label %a [label %b]\na:\n  ret void\nb:\n  ret void\n}}\n",
        ll_header()
    );
    let (m, bodies) = parse(&text);
    match hod_x64::compile_to_object(&m, &bodies, &pic(Stress::default())) {
        Err(hod_x64::ModuleError::Function { error, .. }) => {
            assert_eq!(error, CgError::unsupported(barrier::INLINE_ASM));
        }
        other => panic!("not refused: {:?}", other.map(|o| o.len())),
    }
}

/// A `callbr` result whose only use is a `phi` of a successor is a
/// register value (§4.1), and the shared walk writes a `callbr`'s `phi`s
/// before the lowering, as for every terminator but `invoke`
/// (`crates/hod_cg/src/driver.rs:622`): the result has no location yet,
/// and the walk stops with an internal error before `barrier` can refuse
/// it by name. Not `hod_x64`'s to fix (WP-181's handback); with
/// `CallBr` left out there as `Invoke` is, the lowerings write the
/// edges' `phi`s themselves and both refusals can go.
#[test]
#[ignore = "hod_cg writes a callbr's phis before the result exists (driver.rs:622)"]
fn callbr_result_used_only_by_a_phi() {
    let text = format!(
        "{}define i32 @f(i32 %x) {{\nentry:\n  %r = callbr i32 asm \"\", \"=r,0,!i\"(i32 %x) to label %ft [label %ind]\nind:\n  ret i32 0\nft:\n  %p = phi i32 [ %r, %entry ]\n  ret i32 %p\n}}\n",
        ll_header()
    );
    let (m, bodies) = parse(&text);
    match hod_x64::compile_to_object(&m, &bodies, &pic(Stress::default())) {
        Err(hod_x64::ModuleError::Function { error, .. }) => {
            assert_eq!(error, CgError::unsupported(barrier::RESULT_PHI));
        }
        other => panic!("not refused: {:?}", other.map(|o| o.len())),
    }
}
