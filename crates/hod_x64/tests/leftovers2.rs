//! The x86-64 leftovers, second set (WP-147, `spec/codegen.md` §5.6.16).
//! Each part runs under the emulator against `hod_interp` (where it has
//! the type) and `llc -O0 -fast-isel=0` of LLVM 22 and 19
//! (`common/float_gen.rs`): hod plain, hod stressed, both `llc`s, from
//! one C driver.

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tables of functions and values"
)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use std::fmt::Write as _;

use float_gen::{Check, Suite, T, check, ll_header, parse, pic};
use hod_cg::{CgError, Stress};

const MAJORS: [u32; 2] = [22, 19];
const I64: T = T::I(64);

/// Compiles `text` (functions after the module header) with hod and
/// returns the error, if any.
fn compile(text: &str) -> Result<(), CgError> {
    let (module, bodies) = parse(&format!("{}{text}", ll_header()));
    match hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default())) {
        Ok(_) => Ok(()),
        Err(hod_x64::ModuleError::Function { error, .. }) => Err(error),
        Err(other) => panic!("{other}"),
    }
}

/// Asserts that hod refuses `text` as `name`.
fn refused(text: &str, name: &'static str) {
    assert_eq!(compile(text), Err(CgError::unsupported(name)), "{text}");
}

// ---- 1. `operand:int-of-addr` ----

/// The symbols whose addresses the cases take: (name, a data symbol
/// (offsets apply), whether `hod_interp` knows it).
const SYMS: [(&str, bool, bool); 7] = [
    ("loc", true, true),
    ("def", true, true),
    ("dloc", true, true),
    ("environ", true, false),
    ("wk", true, false),
    ("ifun", false, true),
    ("dfun", false, true),
];

/// The module's symbols. A definition is kept by `llc`'s module only
/// when a call names it, so every function here is called once.
const GLOBALS: &str = "\
@loc = internal global [8 x i64] zeroinitializer, align 8
@def = global [8 x i64] zeroinitializer, align 8
@dloc = dso_local global [8 x i64] zeroinitializer, align 8
@environ = external global ptr, align 8
@wk = extern_weak global i64, align 8
@flag = global i1 true, align 1
define internal i64 @ifun(i64 %x) {
  ret i64 11
}
define i64 @dfun(i64 %x) {
  ret i64 7
}
define i64 @take(i64 %a, i32 %b, i16 %c, i64 %wa, i64 %wb, i64 %wc) {
  %bz = zext i32 %b to i64
  %cz = zext i16 %c to i64
  %x = xor i64 %a, %wa
  %y = xor i64 %bz, %wb
  %z = xor i64 %cz, %wc
  %xy = or i64 %x, %y
  %r = or i64 %xy, %z
  ret i64 %r
}
";

/// The address `@s + k` as a constant `ptr`.
fn addr(s: &str, k: i64) -> String {
    if k == 0 { format!("@{s}") } else { format!("getelementptr (i8, ptr @{s}, i64 {k})") }
}

/// The lines that put `@s + k` as `i64` in `%want` through a `volatile`
/// slot (an independent route), and its low 8, 16, 32 bits.
fn want(s: &str, k: i64) -> String {
    format!(
        "  %slot = alloca ptr, align 8\n  store volatile ptr @{s}, ptr %slot, align 8\n  \
         %base = load volatile ptr, ptr %slot, align 8\n  %bi = ptrtoint ptr %base to i64\n  \
         %want = add i64 %bi, {k}\n  %lo32 = and i64 %want, 4294967295\n  \
         %lo16 = and i64 %want, 65535\n  %lo8 = and i64 %want, 255\n  \
         %c = load volatile i1, ptr @flag, align 1\n  %u = call i64 @ifun(i64 0)\n  \
         %v = call i64 @dfun(i64 0)\n"
    )
}

/// A function body that ORs `got ^ want` over checks, as `%r`.
struct Checks {
    text: String,
    acc: String,
    n: usize,
}

impl Checks {
    fn new(prelude: String) -> Checks {
        Checks { text: prelude, acc: "0".into(), n: 0 }
    }

    /// `lines` define `{g}` and `{w}` (`i64`); `{i}` is a unique suffix.
    fn add(&mut self, lines: &str) {
        let i = self.n;
        self.n += 1;
        let lines = lines
            .replace("{g}", &format!("%g{i}"))
            .replace("{w}", &format!("%w{i}"))
            .replace("{i}", &format!("_{i}"));
        self.text.push_str(&lines);
        let _ = writeln!(
            self.text,
            "  %x{i} = xor i64 %g{i}, %w{i}\n  %o{i} = or i64 {}, %x{i}",
            self.acc
        );
        self.acc = format!("%o{i}");
    }

    fn body(&self) -> String {
        format!("{}  %r = add i64 {}, 0\n", self.text, self.acc)
    }
}

/// Every position a scalar `ptrtoint` of `@s + k` takes as an operand:
/// an instruction's (64, 32, 16, 8 bits), a compare's, `select`'s,
/// `store`'s, a `phi` incoming, call arguments, a returned value.
fn int_of_addr_body(s: &str, k: i64, ret_fn: &str) -> String {
    let a = addr(s, k);
    let mut c = Checks::new(want(s, k));
    c.add(&format!("  {{g}} = xor i64 ptrtoint (ptr {a} to i64), 0\n  {{w}} = add i64 %want, 0\n"));
    c.add(&format!(
        "  %t{{i}} = add i32 ptrtoint (ptr {a} to i32), 0\n  {{g}} = zext i32 %t{{i}} to i64\n  \
         {{w}} = add i64 %lo32, 0\n"
    ));
    c.add(&format!(
        "  %t{{i}} = xor i16 ptrtoint (ptr {a} to i16), 0\n  {{g}} = zext i16 %t{{i}} to i64\n  \
         {{w}} = add i64 %lo16, 0\n"
    ));
    c.add(&format!(
        "  %t{{i}} = or i8 ptrtoint (ptr {a} to i8), 0\n  {{g}} = zext i8 %t{{i}} to i64\n  \
         {{w}} = add i64 %lo8, 0\n"
    ));
    c.add(&format!(
        "  %t{{i}} = trunc i64 %want to i32\n  %e{{i}} = icmp eq i32 %t{{i}}, ptrtoint (ptr {a} to i32)\n  \
         %f{{i}} = icmp ne i64 ptrtoint (ptr {a} to i64), %want\n  %ez{{i}} = zext i1 %e{{i}} to i64\n  \
         %fz{{i}} = zext i1 %f{{i}} to i64\n  {{g}} = sub i64 %ez{{i}}, %fz{{i}}\n  {{w}} = add i64 1, 0\n"
    ));
    c.add(&format!(
        "  %s{{i}} = select i1 %c, i64 ptrtoint (ptr {a} to i64), i64 1\n  {{g}} = add i64 %s{{i}}, 0\n  \
         {{w}} = add i64 %want, 0\n"
    ));
    c.add(&format!(
        "  %m{{i}} = alloca i64, align 8\n  store i64 ptrtoint (ptr {a} to i64), ptr %m{{i}}, align 8\n  \
         {{g}} = load i64, ptr %m{{i}}, align 8\n  {{w}} = add i64 %want, 0\n"
    ));
    c.add(&format!(
        "  %n{{i}} = alloca i32, align 4\n  store i32 ptrtoint (ptr {a} to i32), ptr %n{{i}}, align 4\n  \
         %l{{i}} = load i32, ptr %n{{i}}, align 4\n  {{g}} = zext i32 %l{{i}} to i64\n  {{w}} = add i64 %lo32, 0\n"
    ));
    c.add(&format!(
        "  br i1 %c, label %pa{{i}}, label %pb{{i}}\npa{{i}}:\n  br label %pj{{i}}\npb{{i}}:\n  br label %pj{{i}}\n\
         pj{{i}}:\n  %ph{{i}} = phi i64 [ ptrtoint (ptr {a} to i64), %pa{{i}} ], [ 5, %pb{{i}} ]\n  \
         %pt{{i}} = phi i32 [ ptrtoint (ptr {a} to i32), %pa{{i}} ], [ 5, %pb{{i}} ]\n  \
         %pz{{i}} = zext i32 %pt{{i}} to i64\n  %pw{{i}} = xor i64 %want, %lo32\n  \
         {{g}} = xor i64 %ph{{i}}, %pz{{i}}\n  {{w}} = add i64 %pw{{i}}, 0\n"
    ));
    c.add(&format!(
        "  {{g}} = call i64 @take(i64 ptrtoint (ptr {a} to i64), i32 ptrtoint (ptr {a} to i32), \
         i16 ptrtoint (ptr {a} to i16), i64 %want, i64 %lo32, i64 %lo16)\n  {{w}} = add i64 0, 0\n"
    ));
    c.add(&format!("  {{g}} = call i64 @{ret_fn}(i64 0)\n  {{w}} = add i64 %want, 0\n"));
    c.body()
}

/// §5.6.16 (1): `ptrtoint` of a constant address as an operand
/// (`operand:int-of-addr`), for local, preemptible, `dso_local`,
/// external, weak undefined data and functions, with offsets and
/// truncations, in every operand position; and a vector of class `Fpr`
/// whose constant holds addresses, as an operand and stored. Each
/// function returns 0 when every field equals the address computed from
/// a pointer read through a `volatile` slot.
#[test]
fn constant_operands_of_addresses_agree() {
    let mut suite = Suite::default();
    suite.declare(GLOBALS);
    let mut k = 0;
    for (s, data, interp) in SYMS {
        let offsets: &[i64] = if data { &[0, 8, -24, 4096] } else { &[0] };
        for &off in offsets {
            let check = if interp { Check::Interp } else { Check::Llc };
            let ret_fn = format!("ret_{k}");
            let a = addr(s, off);
            let r = suite.add(
                &ret_fn,
                &[I64],
                I64,
                &format!("  %r = add i64 ptrtoint (ptr {a} to i64), 0\n  br label %b\nb:\n"),
                check,
            );
            // The returned constant itself, not an `add` of it.
            let text = suite.text.split_off(suite.funcs[r].text.start);
            let text = text.replace(
                &format!(
                    "  %r = add i64 ptrtoint (ptr {a} to i64), 0\n  br label %b\nb:\n  ret i64 %r\n"
                ),
                &format!("  ret i64 ptrtoint (ptr {a} to i64)\n"),
            );
            suite.text.push_str(&text);
            suite.funcs[r].text = suite.funcs[r].text.start..suite.text.len();
            let f = suite.add(
                &format!("ioa_{s}_{k}"),
                &[I64],
                I64,
                &int_of_addr_body(s, off, &ret_fn),
                check,
            );
            suite.case(f, &[0]);
            k += 1;
        }
    }
    // Vectors of class `Fpr` whose constants hold addresses.
    for (s, data, interp) in SYMS {
        let check = if interp { Check::Interp } else { Check::Llc };
        let a = addr(s, if data { 16 } else { 0 });
        let mut c = Checks::new(want(s, if data { 16 } else { 0 }));
        c.add(&format!(
            "  %m{{i}} = alloca <1 x i64>, align 8\n  \
             store <1 x i64> <i64 ptrtoint (ptr {a} to i64)>, ptr %m{{i}}, align 8\n  \
             {{g}} = load i64, ptr %m{{i}}, align 8\n  {{w}} = add i64 %want, 0\n"
        ));
        c.add(&format!(
            "  %e{{i}} = extractelement <2 x i64> <i64 5, i64 ptrtoint (ptr {a} to i64)>, i64 %a0\n  \
             %f{{i}} = add i64 %a0, 1\n  \
             %h{{i}} = extractelement <2 x i64> <i64 5, i64 ptrtoint (ptr {a} to i64)>, i64 %f{{i}}\n  \
             %d{{i}} = sub i64 %h{{i}}, %e{{i}}\n  {{g}} = add i64 %d{{i}}, 5\n  {{w}} = add i64 %want, 0\n"
        ));
        c.add(&format!(
            "  %m{{i}} = alloca <2 x ptr>, align 16\n  \
             store <2 x ptr> <ptr {a}, ptr @loc>, ptr %m{{i}}, align 16\n  \
             %p{{i}} = load ptr, ptr %m{{i}}, align 16\n  {{g}} = ptrtoint ptr %p{{i}} to i64\n  \
             {{w}} = add i64 %want, 0\n"
        ));
        c.add(&format!(
            "  %v{{i}} = select i1 %c, <2 x i64> <i64 ptrtoint (ptr {a} to i64), i64 3>, <2 x i64> zeroinitializer\n  \
             %e{{i}} = extractelement <2 x i64> %v{{i}}, i64 0\n  {{g}} = add i64 %e{{i}}, 0\n  \
             {{w}} = add i64 %want, 0\n"
        ));
        let f = suite.add(&format!("vaddr_{s}"), &[I64], I64, &c.body(), check);
        suite.case(f, &[0]);
    }
    check("int-of-addr", &suite, &MAJORS);
}

/// §5.6.16 (1): what has no form is refused by the names AArch64's
/// rule gives: a 4-byte address field in a vector constant (no aligned
/// 8-byte symbol address).
#[test]
fn address_constants_without_a_form_are_refused_by_name() {
    let text = "@g = global i64 0\n\
                define <2 x i32> @f() {\n  ret <2 x i32> <i32 ptrtoint (ptr @g to i32), i32 1>\n}\n";
    refused(text, "x64:constant-relocation");
}
