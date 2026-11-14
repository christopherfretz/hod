//! A `callbr` whose result a successor's `phi` takes (WP-181,
//! `spec/codegen.md` §4.5): the `phi`'s copy is written on the edge, after
//! the template, never before it. Before WP-181 the shared walk wrote it
//! ahead of every terminator but `invoke`, so a loop carrying the result
//! of an `asm goto` read a stale slot (`-15 551286798 -5 ...` where
//! `-12 -8 -4 ...` was meant), and a result used by the `phi` alone
//! stopped with an internal error. Compiled by hod (plain, stressed) and
//! by `llc -O0` of LLVM 22 and 19, linked with one C driver under every
//! linker and run.

mod common;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use common::{assert_refused, emit, ll_header, parse, write_object};
use hod_cg::{Options, Stress};
use hod_testutil::{Scratch, run_with};

/// The functions: the result carried around a loop by the header's `phi`
/// (a value with a second use, kept in a slot), the result taken by the fallthrough's
/// `phi` and used at the indirect destination the template jumps to (a
/// `phi` of the result there is one `llc` 19 and 22 refuse to build: "PHI
/// nodes not grouped"), and an edge named twice
/// whose `phi` takes a value defined before the template.
const FUNCTIONS: &str = r#"
define i32 @loop_home(i32 %x, i32 %n) {
entry:
  br label %head
head:
  %i = phi i32 [ 0, %entry ], [ %i1, %body ]
  %acc = phi i32 [ 0, %entry ], [ %y, %body ]
  %c = icmp slt i32 %i, %n
  br i1 %c, label %body, label %done
body:
  %t = add i32 %acc, %x
  %i1 = add i32 %i, 1
  %y = callbr i32 asm "", "=r,0,!i"(i32 %t) to label %head [label %bad]
bad:
  %b = add i32 %y, -1000
  ret i32 %b
done:
  ret i32 %acc
}

define i32 @to_indirect(i32 %x) {
entry:
  %y = callbr i32 asm "add ${0:w}, ${0:w}, #1\0A cbz ${0:w}, ${2:l}", "=r,0,!i"(i32 %x) to label %ft [label %ind]
ft:
  %f = phi i32 [ %y, %entry ]
  %g = add i32 %f, 1000
  ret i32 %g
ind:
  %q = add i32 %y, 2000
  ret i32 %q
}

define i64 @twice(i64 %x) {
entry:
  %v = mul i64 %x, 3
  callbr void asm "cbz $0, ${1:l}", "r,!i"(i64 %x) to label %join [label %join]
join:
  %p = phi i64 [ %v, %entry ], [ %v, %entry ]
  ret i64 %p
}
"#;

/// The result whose only use is the header's `phi` (a register value).
const LOOP_REG: &str = r#"
define i32 @loop_reg(i32 %x, i32 %n) {
entry:
  br label %head
head:
  %i = phi i32 [ 0, %entry ], [ %i1, %body ]
  %acc = phi i32 [ 0, %entry ], [ %y, %body ]
  %c = icmp slt i32 %i, %n
  br i1 %c, label %body, label %done
body:
  %t = add i32 %acc, %x
  %i1 = add i32 %i, 1
  %y = callbr i32 asm "", "=r,0,!i"(i32 %t) to label %head [label %bad]
bad:
  ret i32 -1000
done:
  ret i32 %acc
}
"#;

const DRIVER: &str = r#"
#include <stdint.h>
#include <stdio.h>
int32_t loop_home(int32_t, int32_t);
int32_t to_indirect(int32_t);
int64_t twice(int64_t);
int32_t loop_reg(int32_t, int32_t);
static int failures;
#define CHECK(e, want) do { long long g_ = (e), w_ = (want); \
  if (g_ != w_) { printf("FAIL %s = %lld, not %lld\n", #e, g_, w_); failures++; } } while (0)
int main(void) {
  for (int32_t x = -3; x <= 3; x++) {
    CHECK(loop_home(x, 4), 4 * x);
    CHECK(loop_home(x, 0), 0);
    CHECK(to_indirect(x), x == -1 ? 2000 : x + 1 + 1000);
    CHECK(twice(x), 3 * x);
    CHECK(loop_reg(x, 4), 4 * x);
    CHECK(loop_reg(x, 0), 0);
  }
  printf("checked, %d failures\n", failures);
  return failures != 0;
}
"#;

fn llc(dir: &Scratch, major: u32) -> Option<PathBuf> {
    let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if !llc.is_file() {
        return None;
    }
    let input =
        dir.write(&format!("llc-{major}.ll"), format!("{}{FUNCTIONS}{LOOP_REG}", ll_header(major)));
    let output = dir.join(format!("llc-{major}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj", "-relocation-model=pic"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llc-{major}: {}", outcome.stderr_text());
    Some(output)
}

#[test]
fn callbr_results_reach_their_phis() {
    let dir = Scratch::new("a64-callbr-phi");
    let (m, bodies) = parse(&format!("{}{FUNCTIONS}{LOOP_REG}", ll_header(22)));
    let mut objects = Vec::new();
    for (name, options) in [
        ("plain", Options::default()),
        ("stress", Options { stress: Stress::ALL, ..Options::default() }),
    ] {
        let emitted = emit(&m, &bodies, &options);
        objects
            .push((format!("hod-{name}"), write_object(&dir, &format!("hod-{name}.o"), &emitted)));
    }
    for major in [22, 19] {
        if let Some(theirs) = llc(&dir, major) {
            objects.push((format!("llc-{major}"), theirs));
        }
    }
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", DRIVER, &[]);
    let mut ran_legs = Vec::new();
    for (who, object) in &objects {
        for linker in &toolchain.linkers {
            let program = dir.join(format!("{who}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, object], &program);
            assert_eq!(
                (exit_code(&ran), ran.stdout_text().as_str()),
                (Some(0), "checked, 0 failures\n"),
                "{who} {}: {}",
                linker.name,
                ran.stderr_text()
            );
            ran_legs.push(format!("{who}/{}", linker.name));
        }
    }
    eprintln!("a64 callbr phi: {ran_legs:?}");
}

/// The loop of [`FUNCTIONS`] with a result whose only use is the
/// header's `phi`: AArch64's `callbr` defines the result again on each
/// indirect edge, after the fallthrough's `phi` released it. Before
/// WP-196 a destination that did not use it left the register held and
/// the block ended with an internal error ("ends with a value in a
/// register"); the edge now lets the result go after its `phi`s.
#[test]
fn a_result_only_a_phi_takes() {
    let (m, bodies) = parse(&format!("{}{LOOP_REG}", ll_header(22)));
    for options in [Options::default(), Options { stress: Stress::ALL, ..Options::default() }] {
        emit(&m, &bodies, &options);
    }
}

/// A result-taking `phi` on an edge the `callbr` names twice: each edge
/// is its own code path, and the walk writes a successor's `phi`s once.
#[test]
fn a_result_on_a_repeated_edge_is_refused_by_name() {
    let twice = "define i32 @f(i32 %x) {\nentry:\n  \
                 %y = callbr i32 asm \"cbz ${0:w}, ${2:l}\", \"=r,0,!i\"(i32 %x) \
                 to label %j [label %j]\nj:\n  %p = phi i32 [ %y, %entry ], [ %y, %entry ]\n  \
                 ret i32 %p\n}\n";
    assert_refused(&[(twice, "call:callbr-result-repeated-edge")]);
}
