//! Native runs and `llc` comparisons of what the skeleton lowers by itself
//! (WP-033): `ret` of a constant and of an argument of every class the
//! ABI returns in registers (and of an aggregate returned in memory),
//! arguments that arrive on the stack, symbol addresses of every access
//! kind. One `.ll` module, compiled by hod, linked with a C driver that
//! checks every function, by every linker. The symbol sequences'
//! relocations are compared with `llc`'s, static, PIC and PIE.

mod common;

use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use common::{emit, ll_header, llc, llc_major, parse, write_object};
use hod_cg::{Options, RelocModel};
use hod_testutil::{run_with, tool};

const FUNCTIONS: &str = r#"
@g = global [4 x i64] [i64 10, i64 20, i64 30, i64 40]
@h = external global i64
@w = extern_weak global i64
@loc = internal global i64 77

define signext i8 @r_i8s() { ret i8 -3 }
define zeroext i8 @r_i8z() { ret i8 200 }
define zeroext i1 @r_i1() { ret i1 true }
define signext i16 @r_i16() { ret i16 -30000 }
define i32 @r_i32() { ret i32 -123456789 }
define i64 @r_i64() { ret i64 81985529216486895 }
define i64 @r_i64n() { ret i64 -2 }
define i64 @r_mask() { ret i64 71777214294589695 }
define ptr @r_null() { ret ptr null }
define i64 @r_undef() { ret i64 undef }
define float @r_f() { ret float 1.5 }
define float @r_f2() { ret float 0x3FB99999A0000000 }
define double @r_d() { ret double 0.1 }
define double @r_d0() { ret double 0.0 }
define half @r_h() { ret half 0xH3C00 }
define fp128 @r_q() { ret fp128 0xL0123456789ABCDEF4000921FB54442D1 }
define i128 @r_i128() { ret i128 -170141183460469231731687303715884105727 }
define { i64, i64 } @r_pair() { ret { i64, i64 } { i64 1, i64 -2 } }
define { double, double } @r_hfa() { ret { double, double } { double 1.0, double 2.5 } }
define { i64, i64, i64 } @r_three() { ret { i64, i64, i64 } { i64 7, i64 8, i64 9 } }
define [9 x i64] @r_big() { ret [9 x i64] [i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, i64 9] }
define <4 x i32> @r_v() { ret <4 x i32> zeroinitializer }

define i64 @a_ninth(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g, i64 %h, i64 %s) { ret i64 %s }
define i64 @a_tenth(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g, i64 %h, i64 %s, i64 %t) { ret i64 %t }
define double @a_second(double %a, double %b) { ret double %b }
define float @a_f9(float %a, float %b, float %c, float %d, float %e, float %f, float %g, float %h, float %s) { ret float %s }
define signext i8 @a_i8(i64 %a, i8 signext %x) { ret i8 %x }
define { i64, i64 } @a_pair({ i64, i64 } %p) { ret { i64, i64 } %p }
define { double, double } @a_hfa({ double, double } %p) { ret { double, double } %p }
define i128 @a_i128(i64 %a, i128 %x) { ret i128 %x }

define ptr @p_g() { ret ptr @g }
define ptr @p_g16() { ret ptr getelementptr (i8, ptr @g, i64 16) }
define ptr @p_h() { ret ptr @h }
define ptr @p_h8() { ret ptr getelementptr (i8, ptr @h, i64 8) }
define ptr @p_w() { ret ptr @w }
define ptr @p_loc() { ret ptr @loc }
define ptr @p_fn() { ret ptr @r_i32 }
define i64 @p_int() { ret i64 ptrtoint (ptr @g to i64) }
"#;

const DRIVER: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <arm_neon.h>

struct pair { int64_t a, b; };
struct hfa { double a, b; };
struct big { int64_t a[9]; };
/* LLVM returns a first-class aggregate of up to eight leaves in
   registers: `{i64,i64,i64}` comes back in x0..x2, which is what C
   sees as a two-register result plus x2 read by a second prototype. */
struct three_lo { int64_t a, b; };

int8_t r_i8s(void); uint8_t r_i8z(void); _Bool r_i1(void); int16_t r_i16(void);
int32_t r_i32(void); int64_t r_i64(void); int64_t r_i64n(void); int64_t r_mask(void);
void *r_null(void); int64_t r_undef(void);
float r_f(void); float r_f2(void); double r_d(void); double r_d0(void); _Float16 r_h(void);
long double r_q(void); __int128 r_i128(void); struct pair r_pair(void); struct hfa r_hfa(void);
struct big r_big(void); struct three_lo r_three(void); int32x4_t r_v(void);
int64_t a_ninth(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
int64_t a_tenth(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
double a_second(double, double);
float a_f9(float, float, float, float, float, float, float, float, float);
int8_t a_i8(int64_t, int8_t);
struct pair a_pair(struct pair); struct hfa a_hfa(struct hfa); __int128 a_i128(int64_t, __int128);
void *p_g(void); void *p_g16(void); void *p_h(void); void *p_h8(void); void *p_w(void);
void *p_loc(void); void *p_fn(void); int64_t p_int(void);

extern int64_t g[4];
int64_t h[2] = { 5, 6 };
__attribute__((weak)) extern int64_t w;

static int fails;
#define CHECK(cond) do { if (!(cond)) { printf("FAIL %s\n", #cond); fails++; } } while (0)

/* The whole x0 after a call, to check the extension of narrow results. */
typedef int64_t (*wide_fn)(void);
static int64_t wide(void *f) { wide_fn g; memcpy(&g, &f, sizeof g); return g(); }

int main(void) {
    CHECK(r_i8s() == -3);
    CHECK(r_i8z() == 200);
    CHECK(r_i1() == 1);
    CHECK(r_i16() == -30000);
    /* AAPCS64 as LLVM implements it: signext/zeroext results are
       extended to 32 bits, an i1 to 8. */
    CHECK((int32_t)wide((void *)r_i8s) == -3);
    CHECK((uint32_t)wide((void *)r_i8z) == 200);
    CHECK((uint8_t)wide((void *)r_i1) == 1);
    CHECK((int32_t)wide((void *)r_i16) == -30000);
    CHECK(r_i32() == -123456789);
    CHECK(r_i64() == 0x0123456789abcdefLL);
    CHECK(r_i64n() == -2);
    CHECK(r_mask() == 0x00ff00ff00ff00ffLL);
    CHECK(r_null() == 0);
    (void)r_undef();
    CHECK(r_f() == 1.5f);
    CHECK(r_f2() == 0.1f);
    CHECK(r_d() == 0.1);
    CHECK(r_d0() == 0.0);
    CHECK(r_h() == (_Float16)1.0);
    long double ld = r_q();
    uint64_t qb[2];
    memcpy(qb, &ld, 16);
    CHECK(qb[0] == 0x0123456789abcdefULL && qb[1] == 0x4000921fb54442d1ULL);
    __int128 i = r_i128();
    CHECK((int64_t)(i >> 64) == INT64_MIN && (uint64_t)i == 1);
    struct pair p = r_pair();
    CHECK(p.a == 1 && p.b == -2);
    struct hfa f = r_hfa();
    CHECK(f.a == 1.0 && f.b == 2.5);
    struct big b = r_big();
    for (int k = 0; k < 9; k++) CHECK(b.a[k] == k + 1);
    struct three_lo t = r_three();
    CHECK(t.a == 7 && t.b == 8);
    int32x4_t v = r_v();
    CHECK(vgetq_lane_s32(v, 0) == 0 && vgetq_lane_s32(v, 3) == 0);

    CHECK(a_ninth(1, 2, 3, 4, 5, 6, 7, 8, 99) == 99);
    CHECK(a_tenth(1, 2, 3, 4, 5, 6, 7, 8, 9, -100) == -100);
    CHECK(a_second(1.0, 2.25) == 2.25);
    CHECK(a_f9(1, 2, 3, 4, 5, 6, 7, 8, 9.5f) == 9.5f);
    CHECK(a_i8(0, -7) == -7);
    struct pair q = a_pair((struct pair){ 11, -12 });
    CHECK(q.a == 11 && q.b == -12);
    struct hfa r = a_hfa((struct hfa){ 0.5, -0.25 });
    CHECK(r.a == 0.5 && r.b == -0.25);
    __int128 x = ((__int128)0x1234 << 64) | 0x5678;
    CHECK(a_i128(0, x) == x);

    CHECK(p_g() == (void *)g);
    CHECK(p_g16() == (void *)&g[2]);
    CHECK(((int64_t *)p_g())[3] == 40);
    CHECK(p_h() == (void *)h);
    CHECK(p_h8() == (void *)&h[1]);
    CHECK(p_w() == (void *)&w);
    CHECK(*(int64_t *)p_loc() == 77);
    CHECK(p_fn() == (void *)r_i32);
    CHECK(p_int() == (int64_t)g);
    printf("checked, %d failures\n", fails);
    return fails;
}
"#;

/// Every function, compiled by hod and called from C, by every linker.
#[test]
fn every_class_returns_and_arrives_natively() {
    let text = format!("{}{FUNCTIONS}", ll_header(22));
    let (m, bodies) = parse(&text);
    let dir = hod_testutil::Scratch::new("a64-native");
    let emitted = emit(&m, &bodies, &Options::default());
    let object = write_object(&dir, "functions.o", &emitted);
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", DRIVER, &[]);
    for linker in &toolchain.linkers {
        let program = dir.join(format!("native-{}", linker.name));
        let ran = toolchain.link_and_run(linker, &[], &[&driver, &object], &program);
        assert_eq!(
            exit_code(&ran),
            Some(0),
            "{}:\n{}{}",
            linker.name,
            ran.stdout_text(),
            ran.stderr_text()
        );
    }
}

/// The relocations of each `p_*` function, in order, as `llvm-objdump
/// -dr` prints them: `TYPE symbol+addend` per line, by function.
fn relocations(object: &std::path::Path) -> Option<Vec<(String, Vec<String>)>> {
    let objdump = tool("llvm-objdump")?;
    let mut command = Command::new(objdump);
    command.args(["-dr", "--no-show-raw-insn"]).arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in outcome.stdout_text().lines() {
        if let Some(name) = line.strip_suffix(">:").and_then(|l| l.split('<').nth(1)) {
            out.push((name.to_string(), Vec::new()));
        } else if line.contains("R_AARCH64_")
            && let Some((_, relocs)) = out.last_mut()
        {
            let fields: Vec<&str> = line.split_whitespace().collect();
            relocs.push(fields[1..].join(" "));
        }
    }
    out.retain(|(name, _)| name.starts_with("p_"));
    Some(out)
}

/// §7.4: each symbol access sequence has `llc`'s relocations, static,
/// PIC and PIC with `PIE Level`.
#[test]
fn symbol_addresses_have_the_relocations_llc_writes() {
    let dir = hod_testutil::Scratch::new("a64-reloc");
    let major = llc_major();
    let pie = "\n!llvm.module.flags = !{!0, !1}\n!0 = !{i32 8, !\"PIC Level\", i32 2}\n!1 = !{i32 7, !\"PIE Level\", i32 2}\n";
    for (name, pic, flags) in [("static", false, ""), ("pic", true, ""), ("pie", true, pie)] {
        let text = format!("{}{FUNCTIONS}{flags}", ll_header(major));
        let reloc = if pic { "-relocation-model=pic" } else { "-relocation-model=static" };
        let Some(theirs) = llc(&dir, &format!("llc-{name}"), &text, &[reloc]) else { return };
        let (m, bodies) = parse(&text);
        let options = Options {
            reloc_model: if pic { RelocModel::Pic } else { RelocModel::Static },
            ..Options::default()
        };
        let ours = write_object(&dir, &format!("hod-{name}.o"), &emit(&m, &bodies, &options));
        let (Some(theirs), Some(ours)) = (relocations(&theirs), relocations(&ours)) else {
            return;
        };
        assert_eq!(ours.len(), 8, "{name}: {ours:?}");
        assert_eq!(ours, theirs, "{name}");
    }
}
