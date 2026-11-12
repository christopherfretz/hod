//! Constant operands that hold addresses (WP-072, `spec/codegen.md`
//! §5.5.1 "Constant operands with addresses"): `ptrtoint` of a constant
//! address (`operand:int-of-addr`), a difference of two symbols
//! (`operand:sym-diff`) and an aggregate constant with an address in it
//! (`operand:address-in-aggregate`), in every position an operand takes
//! (`store`, `insertvalue`, `phi`, `select`, call arguments, `ret`), for
//! symbols of every access (local, preemptible, `dso_local`, hidden,
//! defined elsewhere, weak undefined, functions, block addresses), with
//! offsets and truncations.
//!
//! Each case is a self-checking function that returns 0 when every
//! field it reads back equals the address computed by instructions from
//! a pointer loaded through a `volatile` slot (an independent route),
//! and the C driver checks those pointers against the addresses the C
//! compiler forms itself. The module is built by hod (plain and under
//! `Stress::ALL`) and by `llc -O0 -global-isel=0 -fast-isel=0`, static
//! and PIC, linked as a non-PIE program, a PIE and a shared library under
//! bfd, lld and mold, and run; the cases that name only symbols of the
//! module also run under `hod_interp`.

mod common;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use common::linking::Toolchain;
use common::{emit, ll_header, llc, llc_major, parse, write_object};
use hod_cg::{CgError, Options, RelocModel, Stress};
use hod_interp::{Interp, Program, Val};
use hod_ir::{Arch, Classifier, Support};
use hod_testutil::Scratch;

/// A symbol the cases take the address of.
struct Sym {
    name: &'static str,
    /// A data symbol (offsets apply), else a function.
    data: bool,
    /// Defined in the IR module (`hod_interp` can run its cases).
    local: bool,
}

const SYMS: [Sym; 9] = [
    Sym { name: "loc", data: true, local: true },
    Sym { name: "def", data: true, local: true },
    Sym { name: "dloc", data: true, local: true },
    Sym { name: "cext", data: true, local: false },
    Sym { name: "hid", data: true, local: false },
    Sym { name: "wk", data: true, local: false },
    Sym { name: "ifun", data: false, local: true },
    Sym { name: "dfun", data: false, local: true },
    Sym { name: "cfun", data: false, local: false },
];

/// The module's own symbols and declarations.
const GLOBALS: &str = "\
@loc = internal global [8 x i64] zeroinitializer, align 8
@def = global [8 x i64] zeroinitializer, align 8
@dloc = dso_local global [8 x i64] zeroinitializer, align 8
@cext = external global [8 x i64], align 8
@hid = external hidden global [8 x i64], align 8
@wk = extern_weak global i64, align 8
@flag = global i1 true, align 1
declare i64 @cfun()
define internal i64 @ifun() {
  ret i64 11
}
define i64 @dfun() {
  ret i64 7
}
";

/// The address `@s + k` as a constant of type `ptr`.
fn addr(s: &str, k: i64) -> String {
    if k == 0 { format!("@{s}") } else { format!("getelementptr (i8, ptr @{s}, i64 {k})") }
}

/// A case body: lines that define `%g<i>` (what the constant gave) and
/// `%w<i>` (what the instructions computed), both `i64`.
struct Checks {
    text: String,
    n: usize,
}

impl Checks {
    fn new() -> Checks {
        Checks { text: String::new(), n: 0 }
    }

    /// Adds lines that end with `%g` and `%w` in place of `{g}`, `{w}`.
    fn add(&mut self, lines: &str) {
        let i = self.n;
        self.n += 1;
        let lines = lines.replace("{g}", &format!("%g{i}")).replace("{w}", &format!("%w{i}"));
        let lines = lines.replace("{i}", &format!("_{i}"));
        self.text.push_str(&lines);
    }

    /// The function `name` returning the OR of `g ^ w` over every check.
    fn function(&self, name: &str, prelude: &str) -> String {
        let mut out = format!("define i64 @{name}() {{\nentry:\n{prelude}{}", self.text);
        let mut acc = "0".to_string();
        for i in 0..self.n {
            let _ = writeln!(out, "  %x{i} = xor i64 %g{i}, %w{i}\n  %o{i} = or i64 {acc}, %x{i}");
            acc = format!("%o{i}");
        }
        let _ = writeln!(out, "  ret i64 {acc}\n}}");
        out
    }
}

/// `%want` (`@s + k` as `i64`) and `%twant` (`@loc` as `i64`), from
/// pointers read back through `volatile` slots; `%diff` is
/// `%want - %twant`, or `%want` without `diffs` (`hod_interp` has no
/// symbol differences: there the difference cases are addresses).
fn prelude(s: &str, k: i64, diffs: bool) -> String {
    let diff = if diffs { "sub i64 %want, %twant" } else { "add i64 %want, 0" };
    format!(
        "  %slot = alloca ptr, align 8
  store volatile ptr @{s}, ptr %slot, align 8
  %base = load volatile ptr, ptr %slot, align 8
  %bi = ptrtoint ptr %base to i64
  %want = add i64 %bi, {k}
  store volatile ptr @loc, ptr %slot, align 8
  %tbase = load volatile ptr, ptr %slot, align 8
  %twant = ptrtoint ptr %tbase to i64
  %diff = {diff}
  %lo32 = and i64 %want, 4294967295
  %lo16 = and i64 %want, 65535
  %lo8 = and i64 %want, 255
  %d32 = and i64 %diff, 4294967295
  %c = load volatile i1, ptr @flag, align 1
"
    )
}

/// The cases for `@s + k`: every form, every position.
#[allow(clippy::too_many_lines, reason = "one table of IR text")]
fn cases(s: &str, k: i64, tag: &str, diffs: bool, out: &mut String) -> Vec<String> {
    let a = addr(s, k);
    let int64 = format!("i64 ptrtoint (ptr {a} to i64)");
    let diff64 = if diffs {
        format!("i64 sub (i64 ptrtoint (ptr {a} to i64), i64 ptrtoint (ptr @loc to i64))")
    } else {
        int64.clone()
    };
    let diff32 = format!("i32 trunc ({diff64} to i32)");
    let mut names = Vec::new();

    // Aggregates stored to memory: aligned, packed, truncated, differences.
    let mut c = Checks::new();
    c.add(&format!(
        "  %a{{i}} = alloca {{ ptr, i64 }}, align 8
  store {{ ptr, i64 }} {{ ptr {a}, i64 77 }}, ptr %a{{i}}, align 8
  %p{{i}} = load ptr, ptr %a{{i}}, align 8
  {{g}} = ptrtoint ptr %p{{i}} to i64
  {{w}} = add i64 %want, 0
"
    ));
    c.add(&format!(
        "  %a{{i}} = alloca {{ i64, i64 }}, align 8
  store {{ i64, i64 }} {{ {int64}, i64 0 }}, ptr %a{{i}}, align 8
  %f{{i}} = getelementptr i8, ptr %a{{i}}, i64 8
  %h{{i}} = load i64, ptr %f{{i}}, align 8
  %l{{i}} = load i64, ptr %a{{i}}, align 8
  {{g}} = add i64 %l{{i}}, %h{{i}}
  {{w}} = add i64 %want, 0
"
    ));
    c.add(&format!(
        "  %a{{i}} = alloca <{{ i8, ptr, i8 }}>, align 1
  store <{{ i8, ptr, i8 }}> <{{ i8 5, ptr {a}, i8 9 }}>, ptr %a{{i}}, align 1
  %f{{i}} = getelementptr i8, ptr %a{{i}}, i64 1
  %p{{i}} = load ptr, ptr %f{{i}}, align 1
  %q{{i}} = ptrtoint ptr %p{{i}} to i64
  %e{{i}} = getelementptr i8, ptr %a{{i}}, i64 9
  %b{{i}} = load i8, ptr %e{{i}}, align 1
  %z{{i}} = load i8, ptr %a{{i}}, align 1
  %bz{{i}} = zext i8 %b{{i}} to i64
  %zz{{i}} = zext i8 %z{{i}} to i64
  %s{{i}} = shl i64 %bz{{i}}, 8
  %t{{i}} = or i64 %s{{i}}, %zz{{i}}
  %u{{i}} = xor i64 %t{{i}}, 2309
  {{g}} = add i64 %q{{i}}, %u{{i}}
  {{w}} = add i64 %want, 0
"
    ));
    c.add(&format!(
        "  %a{{i}} = alloca {{ i32, i8, i16, i8, i32 }}, align 4
  store {{ i32, i8, i16, i8, i32 }} {{ i32 ptrtoint (ptr {a} to i32), i8 ptrtoint (ptr {a} to i8), i16 ptrtoint (ptr {a} to i16), i8 -1, i32 -3 }}, ptr %a{{i}}, align 4
  %l{{i}} = load i32, ptr %a{{i}}, align 4
  %f{{i}} = getelementptr i8, ptr %a{{i}}, i64 4
  %b{{i}} = load i8, ptr %f{{i}}, align 1
  %e{{i}} = getelementptr i8, ptr %a{{i}}, i64 6
  %h{{i}} = load i16, ptr %e{{i}}, align 2
  %m{{i}} = getelementptr i8, ptr %a{{i}}, i64 8
  %n{{i}} = load i8, ptr %m{{i}}, align 1
  %k{{i}} = getelementptr i8, ptr %a{{i}}, i64 12
  %j{{i}} = load i32, ptr %k{{i}}, align 4
  %lz{{i}} = zext i32 %l{{i}} to i64
  %bz{{i}} = zext i8 %b{{i}} to i64
  %hz{{i}} = zext i16 %h{{i}} to i64
  %nz{{i}} = zext i8 %n{{i}} to i64
  %jz{{i}} = zext i32 %j{{i}} to i64
  %nx{{i}} = xor i64 %nz{{i}}, 255
  %jx{{i}} = xor i64 %jz{{i}}, 4294967293
  %ex{{i}} = or i64 %nx{{i}}, %jx{{i}}
  %r1{{i}} = shl i64 %bz{{i}}, 32
  %r2{{i}} = shl i64 %hz{{i}}, 40
  %r4{{i}} = or i64 %lz{{i}}, %r1{{i}}
  %r5{{i}} = or i64 %r4{{i}}, %r2{{i}}
  {{g}} = add i64 %r5{{i}}, %ex{{i}}
  %v1{{i}} = shl i64 %lo8, 32
  %v2{{i}} = shl i64 %lo16, 40
  %v4{{i}} = or i64 %lo32, %v1{{i}}
  {{w}} = or i64 %v4{{i}}, %v2{{i}}
"
    ));
    c.add(&format!(
        "  %a{{i}} = alloca <{{ i64, i8, i32 }}>, align 1
  store <{{ i64, i8, i32 }}> <{{ {diff64}, i8 3, {diff32} }}>, ptr %a{{i}}, align 1
  %l{{i}} = load i64, ptr %a{{i}}, align 1
  %f{{i}} = getelementptr i8, ptr %a{{i}}, i64 9
  %h{{i}} = load i32, ptr %f{{i}}, align 1
  %hz{{i}} = zext i32 %h{{i}} to i64
  %t{{i}} = xor i64 %l{{i}}, %hz{{i}}
  {{g}} = add i64 %t{{i}}, 0
  %u{{i}} = xor i64 %diff, %d32
  {{w}} = add i64 %u{{i}}, 0
"
    ));
    // Far from the frame pointer: the stores are rebased.
    c.add(&format!(
        "  %big{{i}} = alloca [6000 x i64], align 8
  store volatile i64 0, ptr %big{{i}}, align 8
  %a{{i}} = alloca <{{ i8, ptr, i16, i8 }}>, align 1
  store <{{ i8, ptr, i16, i8 }}> <{{ i8 1, ptr {a}, i16 ptrtoint (ptr {a} to i16), i8 ptrtoint (ptr {a} to i8) }}>, ptr %a{{i}}, align 1
  %f{{i}} = getelementptr i8, ptr %a{{i}}, i64 1
  %p{{i}} = load ptr, ptr %f{{i}}, align 1
  %q{{i}} = ptrtoint ptr %p{{i}} to i64
  %e{{i}} = getelementptr i8, ptr %a{{i}}, i64 9
  %h{{i}} = load i16, ptr %e{{i}}, align 1
  %hz{{i}} = zext i16 %h{{i}} to i64
  %m{{i}} = getelementptr i8, ptr %a{{i}}, i64 11
  %b{{i}} = load i8, ptr %m{{i}}, align 1
  %bz{{i}} = zext i8 %b{{i}} to i64
  %s{{i}} = shl i64 %bz{{i}}, 16
  %t{{i}} = or i64 %hz{{i}}, %s{{i}}
  %u{{i}} = xor i64 %q{{i}}, %t{{i}}
  {{g}} = add i64 %u{{i}}, 0
  %v1{{i}} = shl i64 %lo8, 16
  %v2{{i}} = or i64 %lo16, %v1{{i}}
  {{w}} = xor i64 %want, %v2{{i}}
"
    ));
    // Scalars: `ptrtoint` and differences as operands of instructions.
    c.add(&format!(
        "  {{g}} = xor {int64}, 0
  {{w}} = add i64 %want, 0
"
    ));
    c.add(&format!(
        "  %t{{i}} = add i32 ptrtoint (ptr {a} to i32), 0
  {{g}} = zext i32 %t{{i}} to i64
  {{w}} = add i64 %lo32, 0
"
    ));
    c.add(&format!(
        "  %t{{i}} = trunc i64 %want to i32
  %e{{i}} = icmp eq i32 %t{{i}}, ptrtoint (ptr {a} to i32)
  {{g}} = zext i1 %e{{i}} to i64
  {{w}} = add i64 1, 0
"
    ));
    c.add(&format!(
        "  {{g}} = add {diff64}, 0
  {{w}} = add i64 %diff, 0
"
    ));
    c.add(&format!(
        "  %t{{i}} = xor {diff32}, 0
  {{g}} = zext i32 %t{{i}} to i64
  {{w}} = add i64 %d32, 0
"
    ));
    c.add(&format!(
        "  %s{{i}} = select i1 %c, {int64}, i64 1
  {{g}} = add i64 %s{{i}}, 0
  {{w}} = add i64 %want, 0
"
    ));
    c.add(&format!(
        "  %slot{{i}} = alloca i64, align 8
  store {int64}, ptr %slot{{i}}, align 8
  {{g}} = load i64, ptr %slot{{i}}, align 8
  {{w}} = add i64 %want, 0
"
    ));
    // `insertvalue` into a constant with an address.
    c.add(&format!(
        "  %iv{{i}} = insertvalue {{ ptr, i64, i32 }} {{ ptr {a}, i64 1, i32 ptrtoint (ptr {a} to i32) }}, i64 %bi, 1
  %p{{i}} = extractvalue {{ ptr, i64, i32 }} %iv{{i}}, 0
  %q{{i}} = ptrtoint ptr %p{{i}} to i64
  %h{{i}} = extractvalue {{ ptr, i64, i32 }} %iv{{i}}, 2
  %hz{{i}} = zext i32 %h{{i}} to i64
  %r{{i}} = extractvalue {{ ptr, i64, i32 }} %iv{{i}}, 1
  %s{{i}} = xor i64 %q{{i}}, %hz{{i}}
  {{g}} = xor i64 %s{{i}}, %r{{i}}
  %t{{i}} = xor i64 %want, %lo32
  {{w}} = xor i64 %t{{i}}, %bi
"
    ));
    // Calls: by value, a narrowed address, a returned aggregate.
    c.add(&format!(
        "  {{g}} = call i64 @take_{tag}({{ ptr, i64 }} {{ ptr {a}, i64 3 }}, i32 ptrtoint (ptr {a} to i32), {{ i32, i32 }} {{ i32 ptrtoint (ptr {a} to i32), {diff32} }}, i64 %want, i64 %diff)
  {{w}} = add i64 0, 0
"
    ));
    c.add(
        "  %r{i} = call { ptr, i64 } @give_TAG()
  %p{i} = extractvalue { ptr, i64 } %r{i}, 0
  %q{i} = ptrtoint ptr %p{i} to i64
  %h{i} = extractvalue { ptr, i64 } %r{i}, 1
  {g} = add i64 %q{i}, %h{i}
  %t{i} = add i64 %want, %diff
  {w} = add i64 %t{i}, 0
"
        .replace("TAG", tag)
        .as_str(),
    );
    let name = format!("case_{tag}");
    out.push_str(&c.function(&name, &prelude(s, k, diffs)));
    names.push(name);
    let _ = write!(
        out,
        "define internal i64 @take_{tag}({{ ptr, i64 }} %p, i32 %n, {{ i32, i32 }} %h, i64 %want, i64 %diff) {{
entry:
  %a = extractvalue {{ ptr, i64 }} %p, 0
  %ai = ptrtoint ptr %a to i64
  %x = xor i64 %ai, %want
  %b = extractvalue {{ ptr, i64 }} %p, 1
  %bx = xor i64 %b, 3
  %nz = zext i32 %n to i64
  %wn = and i64 %want, 4294967295
  %nx = xor i64 %nz, %wn
  %h0 = extractvalue {{ i32, i32 }} %h, 0
  %h1 = extractvalue {{ i32, i32 }} %h, 1
  %h0z = zext i32 %h0 to i64
  %h1z = zext i32 %h1 to i64
  %dn = and i64 %diff, 4294967295
  %h0x = xor i64 %h0z, %wn
  %h1x = xor i64 %h1z, %dn
  %o1 = or i64 %x, %bx
  %o2 = or i64 %o1, %nx
  %o3 = or i64 %o2, %h0x
  %o4 = or i64 %o3, %h1x
  ret i64 %o4
}}
define internal {{ ptr, i64 }} @give_{tag}() {{
entry:
  ret {{ ptr, i64 }} {{ ptr {a}, {diff64} }}
}}
"
    );

    // `phi`s of constants with addresses.
    let _ = write!(
        out,
        "define i64 @phi_{tag}() {{
entry:
{pre}  br i1 %c, label %one, label %two
one:
  br label %join
two:
  br label %join
join:
  %p = phi ptr [ {a}, %one ], [ {a}, %two ]
  %pn = phi i32 [ ptrtoint (ptr {a} to i32), %one ], [ ptrtoint (ptr {a} to i32), %two ]
  %q = phi i64 [ {u64}, %one ], [ {u64}, %two ]
  %r = phi i64 [ {ud64}, %one ], [ {ud64}, %two ]
  %s = phi i32 [ {ud32}, %one ], [ {ud32}, %two ]
  %p0i = ptrtoint ptr %p to i64
  %p1z = zext i32 %pn to i64
  %sz = zext i32 %s to i64
  %x0 = xor i64 %p0i, %want
  %x1 = xor i64 %p1z, %lo32
  %x2 = xor i64 %q, %want
  %x3 = xor i64 %r, %diff
  %x4 = xor i64 %sz, %d32
  %o1 = or i64 %x0, %x1
  %o2 = or i64 %o1, %x2
  %o3 = or i64 %o2, %x3
  %o4 = or i64 %o3, %x4
  ret i64 %o4
}}
",
        pre = prelude(s, k, diffs),
        u64 = &int64[4..],
        ud64 = &diff64[4..],
        ud32 = &diff32[4..],
    );
    names.push(format!("phi_{tag}"));
    names
}

/// Block addresses and `dso_local_equivalent` in aggregates, jumped and
/// called through.
fn special_cases(out: &mut String, local_only: bool) -> Vec<String> {
    out.push_str(
        "define i64 @block_check() {
entry:
  %a = alloca <{ i8, ptr }>, align 1
  store <{ i8, ptr }> <{ i8 1, ptr blockaddress(@block_check, %yes) }>, ptr %a, align 1
  %f = getelementptr i8, ptr %a, i64 1
  %p = load ptr, ptr %f, align 1
  %b = alloca { i64, ptr, i32 }, align 8
  store { i64, ptr, i32 } { i64 5, ptr blockaddress(@block_check, %no), i32 ptrtoint (ptr blockaddress(@block_check, %bad) to i32) }, ptr %b, align 8
  %g = getelementptr i8, ptr %b, i64 8
  %q = load ptr, ptr %g, align 8
  %c = load volatile i1, ptr @flag, align 1
  %t = select i1 %c, ptr %p, ptr %q
  indirectbr ptr %t, [label %yes, label %no]
yes:
  indirectbr ptr %q, [label %no, label %bad]
no:
  %r = phi i64 [ 0, %yes ], [ 1, %entry ]
  ret i64 %r
bad:
  ret i64 2
}
",
    );
    let names = vec!["block_check".to_string(), "equiv_case".to_string()];
    let callee = if local_only { "dfun" } else { "cfun" };
    let want = if local_only { 7 } else { 42 };
    let _ = write!(
        out,
        "define i64 @equiv_case() {{
entry:
  %a = alloca {{ i8, ptr, ptr }}, align 8
  store {{ i8, ptr, ptr }} {{ i8 0, ptr dso_local_equivalent @{callee}, ptr dso_local_equivalent @ifun }}, ptr %a, align 8
  %f = getelementptr i8, ptr %a, i64 8
  %p = load ptr, ptr %f, align 8
  %r = call i64 %p()
  %g = getelementptr i8, ptr %a, i64 16
  %q = load ptr, ptr %g, align 8
  %s = call i64 %q()
  %x = xor i64 %r, {want}
  %y = xor i64 %s, 11
  %o = or i64 %x, %y
  ret i64 %o
}}
"
    );
    names
}

/// The module text of every case, and the case functions' names. With
/// `local_only`, only symbols the module defines (for `hod_interp`).
fn module_text(major: u32, local_only: bool) -> (String, Vec<String>) {
    let mut out = ll_header(major);
    out.push_str(GLOBALS);
    let mut names = Vec::new();
    for sym in SYMS.iter().filter(|s| s.local || !local_only) {
        let offsets: &[i64] = if sym.data { &[0, 24, -8, 4099] } else { &[0] };
        for &k in offsets {
            let tag =
                format!("{}_{}", sym.name, if k < 0 { format!("m{}", -k) } else { k.to_string() });
            names.extend(cases(sym.name, k, &tag, !local_only, &mut out));
        }
    }
    names.extend(special_cases(&mut out, local_only));
    if !local_only {
        // The C driver compares these with its own addresses.
        for sym in &SYMS {
            let _ = writeln!(out, "define ptr @addr_{0}() {{\n  ret ptr @{0}\n}}", sym.name);
        }
    }
    (out, names)
}

/// The C side: `cext` and `cfun` (in the program), `hid` (next to the
/// module), and `main`, which runs every case and checks the addresses.
fn driver(names: &[String]) -> String {
    let mut out = String::from(
        "#include <stdio.h>\nlong long cext[8];\nlong cfun(void) { return 42; }\n\
         extern long long def[8], dloc[8];\nlong dfun(void);\n",
    );
    for name in names {
        let _ = writeln!(out, "long {name}(void);");
    }
    for sym in &SYMS {
        let _ = writeln!(out, "void *addr_{}(void);", sym.name);
    }
    out.push_str(
        "void *hid_address(void);\n__attribute__((weak)) extern long long wk;\n\
         int main(void) {\n  int bad = 0;\n  long r;\n",
    );
    for name in names {
        let _ =
            writeln!(out, "  r = {name}();\n  if (r) {{ printf(\"{name}: %lx\\n\", r); bad++; }}");
    }
    out.push_str(
        "  if (addr_cext() != (void *)cext) { puts(\"cext\"); bad++; }\n\
         if (addr_cfun() != (void *)cfun) { puts(\"cfun\"); bad++; }\n\
         if (addr_def() != (void *)def) { puts(\"def\"); bad++; }\n\
         if (addr_dfun() != (void *)dfun) { puts(\"dfun\"); bad++; }\n\
         if (addr_wk() != (void *)&wk || addr_wk() != 0) { puts(\"wk\"); bad++; }\n\
         if (addr_hid() != hid_address()) { puts(\"hid\"); bad++; }\n\
         printf(\"%d failed\\n\", bad);\n  return bad != 0;\n}\n",
    );
    out
}

/// Defined next to the module (in the shared library when there is one).
const HIDDEN: &str = "__attribute__((visibility(\"hidden\"))) long long hid[8];\n\
                      void *hid_address(void) { return hid; }\n";

/// The cases pass the classifier with the shipped matrix: every form is
/// accepted, none is a construct the matrix declines.
#[test]
fn the_cases_are_accepted_by_classify() {
    let (text, _) = module_text(22, false);
    let (module, bodies) = parse(&text);
    let classifier = Classifier::new(Support::shipped(Arch::Aarch64));
    classifier.module(&module).unwrap_or_else(|e| panic!("{e:?}"));
    for body in &bodies {
        classifier.body(&module, body).unwrap_or_else(|e| panic!("{e:?}"));
    }
}

/// Every case of the module's own symbols returns 0 under `hod_interp`.
#[test]
fn the_cases_agree_with_the_interpreter() {
    let (text, names) = module_text(22, true);
    let (module, bodies) = parse(&text);
    let program = Program::with_bodies(&module, &bodies).expect("a valid program");
    let mut ran = 0;
    for name in &names {
        let sym = module.find_symbol(name.as_bytes()).expect("the case is defined");
        let mut interp = Interp::new(&program);
        hod_interp::host::register(&mut interp);
        let outcome = interp.run(sym, &[]);
        match outcome.result {
            Ok(Some(Val::Int { value: 0, .. })) => ran += 1,
            other => panic!("{name}: {other:?}"),
        }
    }
    eprintln!("constaddr: {ran} cases agree with hod_interp");
    assert_eq!(ran, names.len());
}

/// Object files of the module from hod (plain and stressed) and `llc`.
fn objects(dir: &Scratch, text: &str, pic: bool) -> Vec<(&'static str, PathBuf)> {
    let (module, bodies) = parse(text);
    let reloc_model = if pic { RelocModel::Pic } else { RelocModel::Static };
    let mut out = Vec::new();
    let suffix = if pic { "pic" } else { "static" };
    for (label, stress) in [("hod", Stress::default()), ("hod-stress", Stress::ALL)] {
        let options = Options { reloc_model, stress, ..Options::default() };
        let emitted = emit(&module, &bodies, &options);
        out.push((label, write_object(dir, &format!("{label}-{suffix}.o"), &emitted)));
    }
    let model = if pic { "-relocation-model=pic" } else { "-relocation-model=static" };
    if let Some(object) = llc(dir, &format!("llc-{suffix}"), text, &[model]) {
        out.push(("llc", object));
    }
    out
}

/// Links and runs every object under every linker: non-PIE (static
/// code), PIE and a shared library (PIC code).
#[test]
fn the_cases_run_under_every_linker() {
    let dir = Scratch::new("hod-a64-constaddr");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let (text, names) = module_text(llc_major(), false);
    let main_c = driver(&names);
    let mut runs = 0;
    for pic in [false, true] {
        let pie_flags: &[&str] = if pic { &["-fPIE"] } else { &["-fno-pie"] };
        let suffix = if pic { "pic" } else { "static" };
        let main_o = toolchain.compile_c(&dir, &format!("main-{suffix}"), &main_c, pie_flags);
        let lib_flags: &[&str] = if pic { &["-fPIC"] } else { &["-fno-pie"] };
        let hid_o = toolchain.compile_c(&dir, &format!("hid-{suffix}"), HIDDEN, lib_flags);
        for (label, object) in objects(&dir, &text, pic) {
            for linker in &toolchain.linkers {
                let name = format!("{label}-{suffix}-{}", linker.name);
                if pic {
                    run_ok(
                        &toolchain,
                        linker,
                        &dir,
                        &format!("{name}-pie"),
                        &["-pie"],
                        &[&main_o, &object, &hid_o],
                    );
                    let lib = dir.join(format!("lib{name}.so"));
                    toolchain.link_ok(linker, &["-shared"], &[&object, &hid_o], &lib);
                    let rpath = format!("-Wl,-rpath,{}", dir.path().display());
                    run_ok(
                        &toolchain,
                        linker,
                        &dir,
                        &format!("{name}-so"),
                        &["-pie", &rpath],
                        &[&main_o, &lib],
                    );
                    runs += 2;
                } else {
                    run_ok(
                        &toolchain,
                        linker,
                        &dir,
                        &name,
                        &["-no-pie"],
                        &[&main_o, &object, &hid_o],
                    );
                    runs += 1;
                }
            }
        }
    }
    eprintln!("constaddr: {} cases, {runs} linked programs, all 0 failed", names.len());
}

fn run_ok(
    toolchain: &Toolchain,
    linker: &common::linking::Linker,
    dir: &Scratch,
    name: &str,
    flags: &[&str],
    inputs: &[&Path],
) {
    let program = dir.join(name);
    let ran = toolchain.link_and_run(linker, flags, inputs, &program);
    let stdout = ran.stdout_text();
    assert!(
        ran.success() && stdout.ends_with("0 failed\n"),
        "{name}: {stdout}{}",
        ran.stderr_text()
    );
}

/// What stays refused, each by its one name.
#[test]
fn what_stays_refused_has_one_name() {
    let compile = |functions: &str| -> Result<(), CgError> {
        let (m, bodies) = parse(&format!("{}{GLOBALS}{functions}", ll_header(22)));
        common::chunk(&m, &bodies, &Options::default()).map(|_| ())
    };
    // An address in an integer wider than a register has no image.
    assert_eq!(
        compile(
            "define void @f(ptr %p) {\n  store i128 ptrtoint (ptr @loc to i128), ptr %p, align 16\n  ret void\n}\n"
        ),
        Err(CgError::unsupported(hod_a64::common::CONSTANT))
    );
    // A vector constant with a narrowed address (vectors are not WP-072's).
    assert_eq!(
        compile(
            "define <2 x i32> @f() {\n  ret <2 x i32> <i32 ptrtoint (ptr @loc to i32), i32 0>\n}\n"
        ),
        Err(CgError::unsupported(hod_a64::common::CONSTANT_RELOC))
    );
}

/// `hod_ll` does not read an aggregate constant as a `phi`'s incoming
/// value (`crates/hod_ll/src/index.rs`, the value scanner, accepts no
/// `{`, `[` or `<`), which LLVM accepts. Not WP-072's code: recorded here.
#[test]
#[ignore = "hod_ll bug: an aggregate constant as a phi incoming value is a syntax error (crates/hod_ll/src/index.rs:463)"]
fn hod_ll_reads_an_aggregate_phi() {
    let text = format!(
        "{}{GLOBALS}define i64 @f(i1 %c) {{\nentry:\n  br i1 %c, label %a, label %b\na:\n  br label %j\nb:\n  br label %j\nj:\n  %p = phi {{ ptr, i64 }} [ {{ ptr @loc, i64 1 }}, %a ], [ {{ ptr @def, i64 2 }}, %b ]\n  %r = extractvalue {{ ptr, i64 }} %p, 1\n  ret i64 %r\n}}\n",
        ll_header(22)
    );
    let (module, bodies) = parse(&text);
    assert_eq!(bodies.len(), 3, "{}", module.symbol_count());
}
