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

// ---- 2. `llvm.eh.sjlj.setjmp` and `llvm.eh.sjlj.longjmp` ----

/// The functions that set the buffer, as clang writes `__builtin_setjmp`
/// (the frame address to word 0, the stack pointer to word 2, then the
/// intrinsic). Values cross the `setjmp` in memory, as at `-O0`.
const SETTERS: &str = "\
@gbuf = external global [5 x ptr], align 16
declare ptr @llvm.frameaddress.p0(i32)
declare ptr @llvm.stacksave.p0()
declare i32 @llvm.eh.sjlj.setjmp(ptr)
declare void @llvm.eh.sjlj.longjmp(ptr)

; Sets `buf`, calls `cb(buf, v + 1)`, which jumps back: (v + 1) * 3 + 1.
define i64 @catch_once(ptr %buf, ptr %cb, i64 %v) {
entry:
  %x = alloca i64, align 8
  store i64 %v, ptr %x, align 8
  %fa = call ptr @llvm.frameaddress.p0(i32 0)
  store ptr %fa, ptr %buf, align 8
  %ss = call ptr @llvm.stacksave.p0()
  %p2 = getelementptr ptr, ptr %buf, i64 2
  store ptr %ss, ptr %p2, align 8
  %r = call i32 @llvm.eh.sjlj.setjmp(ptr %buf)
  %z = icmp eq i32 %r, 0
  br i1 %z, label %first, label %second
first:
  %l = load i64, ptr %x, align 8
  %n = add i64 %l, 1
  store i64 %n, ptr %x, align 8
  call void %cb(ptr %buf, i64 %n)
  ret i64 -1
second:
  %w = load i64, ptr %x, align 8
  %m = mul i64 %w, 3
  %rz = zext i32 %r to i64
  %s = add i64 %m, %rz
  ret i64 %s
}

; The same through the global buffer and `cb(v)`.
define i64 @catch_global(ptr %cb, i64 %v) {
entry:
  %x = alloca i64, align 8
  store i64 %v, ptr %x, align 8
  %fa = call ptr @llvm.frameaddress.p0(i32 0)
  store ptr %fa, ptr @gbuf, align 16
  %ss = call ptr @llvm.stacksave.p0()
  store ptr %ss, ptr getelementptr inbounds (ptr, ptr @gbuf, i64 2), align 16
  %r = call i32 @llvm.eh.sjlj.setjmp(ptr @gbuf)
  %z = icmp ne i32 %r, 0
  br i1 %z, label %second, label %first
first:
  %l = load i64, ptr %x, align 8
  %n = add i64 %l, 1
  store i64 %n, ptr %x, align 8
  call void %cb(i64 %n)
  ret i64 -1
second:
  %w = load i64, ptr %x, align 8
  %m = mul i64 %w, 3
  %rz = zext i32 %r to i64
  %s = add i64 %m, %rz
  ret i64 %s
}

; A dynamic `alloca` before the `setjmp`, which is jumped to `times - 1`
; times: c * 100 + r * 10 + the byte, c = times.
define i64 @catch_loop(ptr %buf, ptr %cb, i64 %times) {
entry:
  %k = alloca i64, align 8
  store i64 0, ptr %k, align 8
  %dyn = alloca i8, i64 %times, align 16
  store volatile i8 7, ptr %dyn, align 1
  %fa = call ptr @llvm.frameaddress.p0(i32 0)
  store ptr %fa, ptr %buf, align 8
  %ss = call ptr @llvm.stacksave.p0()
  %p2 = getelementptr ptr, ptr %buf, i64 2
  store ptr %ss, ptr %p2, align 8
  %r = call i32 @llvm.eh.sjlj.setjmp(ptr %buf)
  %c = load i64, ptr %k, align 8
  %c1 = add i64 %c, 1
  store i64 %c1, ptr %k, align 8
  %more = icmp ult i64 %c1, %times
  br i1 %more, label %again, label %out
again:
  %big = alloca i8, i64 %c1, align 16
  store volatile i8 1, ptr %big, align 1
  call void %cb(ptr %buf, i64 %c1)
  ret i64 -1
out:
  %d = load volatile i8, ptr %dyn, align 1
  %dz = zext i8 %d to i64
  %rz = zext i32 %r to i64
  %h = mul i64 %c1, 100
  %t = mul i64 %rz, 10
  %ht = add i64 %h, %t
  %s = add i64 %ht, %dz
  ret i64 %s
}

; The jump from the function that set the buffer: v * 3 + 1.
define i64 @self_jump(ptr %buf, i64 %v) {
entry:
  %fa = call ptr @llvm.frameaddress.p0(i32 0)
  store ptr %fa, ptr %buf, align 8
  %ss = call ptr @llvm.stacksave.p0()
  %p2 = getelementptr ptr, ptr %buf, i64 2
  store ptr %ss, ptr %p2, align 8
  %r = call i32 @llvm.eh.sjlj.setjmp(ptr %buf)
  %z = icmp eq i32 %r, 0
  br i1 %z, label %first, label %second
first:
  call void @llvm.eh.sjlj.longjmp(ptr %buf)
  unreachable
second:
  %m = mul i64 %v, 3
  %rz = zext i32 %r to i64
  %s = add i64 %m, %rz
  ret i64 %s
}
";

/// The functions that jump.
const JUMPERS: &str = "\
@gbuf = external global [5 x ptr], align 16
declare void @llvm.eh.sjlj.longjmp(ptr)

define void @jump(ptr %buf, i64 %v) {
  call void @llvm.eh.sjlj.longjmp(ptr %buf)
  unreachable
}

define void @jump_global(i64 %v) {
  call void @llvm.eh.sjlj.longjmp(ptr @gbuf)
  unreachable
}
";

/// The C driver: each call made by `call_checked`, which puts known
/// values in `rbx`, `rbp`, `r12` to `r15`, makes the call, and sets bit
/// 32 of the result when one of them differs afterwards;
/// `clobber_and_jump` writes other values into the same registers before
/// it calls `jump` (nothing restores them but the setter's epilogue).
const SJLJ_DRIVER: &str = r#"
#include <stdio.h>
#include <stdint.h>
typedef uint64_t u;
u catch_once(void **, void (*)(void **, u), u);
u catch_global(void (*)(u), u);
u catch_loop(void **, void (*)(void **, u), u);
u self_jump(void **, u);
void jump(void **, u);
void jump_global(u);
void clobber_and_jump(void **, u);
u call_checked(void *, u, u, u);
void *gbuf[5];
__asm__(
  ".text\n"
  ".globl call_checked\n"
  "call_checked:\n"
  "  pushq %rbx\n  pushq %rbp\n  pushq %r12\n  pushq %r13\n  pushq %r14\n  pushq %r15\n"
  "  subq $8, %rsp\n"
  "  movq %rdi, %rax\n  movq %rsi, %rdi\n  movq %rdx, %rsi\n  movq %rcx, %rdx\n"
  "  movabsq $0x1111111111111111, %rbx\n  movabsq $0x2222222222222222, %rbp\n"
  "  movabsq $0x3333333333333333, %r12\n  movabsq $0x4444444444444444, %r13\n"
  "  movabsq $0x5555555555555555, %r14\n  movabsq $0x6666666666666666, %r15\n"
  "  callq *%rax\n"
  "  xorl %ecx, %ecx\n"
  "  movabsq $0x1111111111111111, %rdx\n  xorq %rdx, %rbx\n  orq %rbx, %rcx\n"
  "  movabsq $0x2222222222222222, %rdx\n  xorq %rdx, %rbp\n  orq %rbp, %rcx\n"
  "  movabsq $0x3333333333333333, %rdx\n  xorq %rdx, %r12\n  orq %r12, %rcx\n"
  "  movabsq $0x4444444444444444, %rdx\n  xorq %rdx, %r13\n  orq %r13, %rcx\n"
  "  movabsq $0x5555555555555555, %rdx\n  xorq %rdx, %r14\n  orq %r14, %rcx\n"
  "  movabsq $0x6666666666666666, %rdx\n  xorq %rdx, %r15\n  orq %r15, %rcx\n"
  "  movl %eax, %eax\n"
  "  testq %rcx, %rcx\n  je 1f\n  btsq $32, %rax\n"
  "1:\n  addq $8, %rsp\n"
  "  popq %r15\n  popq %r14\n  popq %r13\n  popq %r12\n  popq %rbp\n  popq %rbx\n  ret\n"
  ".globl clobber_and_jump\n"
  "clobber_and_jump:\n"
  "  pushq %rbx\n  pushq %r12\n  pushq %r13\n  pushq %r14\n  pushq %r15\n"
  "  movq $-1, %rbx\n  movq $-2, %r12\n  movq $-3, %r13\n  movq $-4, %r14\n  movq $-5, %r15\n"
  "  movq $-6, %rbp\n"
  "  callq jump\n"
  "  ud2\n"
);
static void global_cb(u v) { (void)v; jump_global(v); }
int main(void) {
  void *buf[5];
  printf("%llx\n", (unsigned long long)call_checked((void *)catch_once, (u)buf, (u)jump, 41));
  printf("%llx\n", (unsigned long long)call_checked((void *)catch_once, (u)buf, (u)clobber_and_jump, 41));
  printf("%llx\n", (unsigned long long)call_checked((void *)catch_global, (u)global_cb, 9, 0));
  printf("%llx\n", (unsigned long long)call_checked((void *)catch_loop, (u)buf, (u)clobber_and_jump, 5));
  printf("%llx\n", (unsigned long long)call_checked((void *)catch_loop, (u)buf, (u)jump, 1));
  printf("%llx\n", (unsigned long long)call_checked((void *)self_jump, (u)buf, 9, 0));
  return 0;
}
"#;

/// What [`SJLJ_DRIVER`] prints.
const SJLJ_OUTPUT: &str = "7f\n7f\n1f\n205\n6b\n1c\n";

/// hod's object of `text` (header added) with `stress`.
fn hod_object(
    scratch: &hod_testutil::Scratch,
    name: &str,
    text: &str,
    stress: Stress,
) -> std::path::PathBuf {
    let (module, bodies) = parse(&format!("{}{text}", ll_header()));
    let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    scratch.write(&format!("{name}.o"), object)
}

/// §5.6.16 (2): `__builtin_setjmp` and `__builtin_longjmp` with `llc`'s
/// x86-64 buffer layout, so that the objects interoperate: every
/// pairing of the setters (hod plain and stressed, `llc` 22 and 19) with
/// the jumpers (hod, `llc` 22 and 19), linked by every linker, prints the
/// same; the save set the caller had is intact after a jump that
/// clobbered it.
#[test]
fn sjlj_interoperates_with_llc() {
    let scratch = hod_testutil::Scratch::new("x64-sjlj");
    let mut setters = vec![
        ("hod".to_string(), hod_object(&scratch, "set-hod", SETTERS, Stress::default())),
        ("hod-stress".to_string(), hod_object(&scratch, "set-stress", SETTERS, Stress::ALL)),
    ];
    let mut jumpers =
        vec![("hod".to_string(), hod_object(&scratch, "jump-hod", JUMPERS, Stress::default()))];
    for major in MAJORS {
        let text = format!("{}{SETTERS}", ll_header());
        if let Some(o) = float_gen::llc(&scratch, major, &format!("set-llc{major}"), &text) {
            setters.push((format!("llc{major}"), o));
        }
        let text = format!("{}{JUMPERS}", ll_header());
        if let Some(o) = float_gen::llc(&scratch, major, &format!("jump-llc{major}"), &text) {
            jumpers.push((format!("llc{major}"), o));
        }
    }
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    let driver = toolchain.compile_c(&scratch, "sjlj-driver", SJLJ_DRIVER, &[]);
    let mut runs = 0;
    for (s, set) in &setters {
        for (j, jump) in &jumpers {
            for linker in &toolchain.linkers {
                let program = scratch.join(format!("sjlj-{s}-{j}-{}", linker.name));
                let ran = toolchain.link_and_run(linker, &[], &[&driver, set, jump], &program);
                assert!(
                    ran.success(),
                    "{s} + {j} [{}]: {:?}\n{}",
                    linker.name,
                    ran.status,
                    ran.stderr_text()
                );
                assert_eq!(
                    ran.stdout_text(),
                    SJLJ_OUTPUT,
                    "setters {s}, jumpers {j} [{}]",
                    linker.name
                );
                runs += 1;
            }
        }
    }
    eprintln!("sjlj: {} setters x {} jumpers, {runs} programs", setters.len(), jumpers.len());
}

/// §5.6.16 (2): the refusals, by this target's names.
#[test]
fn sjlj_refusals_by_name() {
    use hod_x64::intrinsic::{SJLJ_LIVE_VALUE, SJLJ_SHADOW_STACK};
    let decls = "declare i32 @llvm.eh.sjlj.setjmp(ptr)\ndeclare void @llvm.eh.sjlj.longjmp(ptr)\n";
    // A register value of the block used after the `setjmp`: an
    // instruction's result, and an argument of the entry block.
    refused(
        &format!(
            "{decls}define i32 @f(ptr %b, i32 %a) {{\n  %x = add i32 %a, 1\n  \
             %r = call i32 @llvm.eh.sjlj.setjmp(ptr %b)\n  %s = add i32 %x, %r\n  ret i32 %s\n}}\n"
        ),
        SJLJ_LIVE_VALUE,
    );
    refused(
        &format!(
            "{decls}define i32 @f(ptr %b, i32 %a) {{\n  %r = call i32 @llvm.eh.sjlj.setjmp(ptr %b)\n  \
             %s = add i32 %a, %r\n  ret i32 %s\n}}\n"
        ),
        SJLJ_LIVE_VALUE,
    );
    // Used before it, or in another block (a home value): compiled.
    assert_eq!(
        compile(&format!(
            "{decls}define i32 @f(ptr %b, i32 %a) {{\n  %x = add i32 %a, 1\n  store i32 %x, ptr %b\n  \
             %r = call i32 @llvm.eh.sjlj.setjmp(ptr %b)\n  br label %n\nn:\n  %s = add i32 %a, %r\n  ret i32 %s\n}}\n"
        )),
        Ok(())
    );
    // A shadow stack.
    let shstk = "!llvm.module.flags = !{!0}\n!0 = !{i32 8, !\"cf-protection-return\", i32 1}\n";
    refused(
        &format!(
            "{decls}define void @g(ptr %b) {{\n  call void @llvm.eh.sjlj.longjmp(ptr %b)\n  unreachable\n}}\n{shstk}"
        ),
        SJLJ_SHADOW_STACK,
    );
}

/// `F`: a `setjmp` function that holds nothing in a callee-saved
/// register. It keeps the jumper `g` in a global (an argument used after
/// the `setjmp` is refused), calls `g(buf)`, and returns after the
/// second return, counting it in `@resumed`.
const SAVE_ALL_SETTER: &str = "\
@gsel = global ptr null, align 8
@resumed = global i32 0, align 4
declare ptr @llvm.frameaddress.p0(i32)
declare ptr @llvm.stacksave.p0()
declare i32 @llvm.eh.sjlj.setjmp(ptr)

define void @f_sj(ptr %g) {
entry:
  %buf = alloca [5 x ptr], align 16
  store ptr %g, ptr @gsel, align 8
  %fa = call ptr @llvm.frameaddress.p0(i32 0)
  store ptr %fa, ptr %buf, align 16
  %ss = call ptr @llvm.stacksave.p0()
  %p2 = getelementptr ptr, ptr %buf, i64 2
  store ptr %ss, ptr %p2, align 16
  %r = call i32 @llvm.eh.sjlj.setjmp(ptr %buf)
  %z = icmp eq i32 %r, 0
  br i1 %z, label %first, label %second
first:
  %gg = load ptr, ptr @gsel, align 8
  call void %gg(ptr %buf)
  ret void
second:
  %n = load i32, ptr @resumed, align 4
  %n1 = add i32 %n, 1
  store i32 %n1, ptr @resumed, align 4
  ret void
}
";

/// `G` by hod: five values held in callee-saved registers across a call
/// of `touch`, then `longjmp` to `buf`.
const SAVE_ALL_HOD_JUMPER: &str = "\
declare void @touch(i64)
declare void @llvm.eh.sjlj.longjmp(ptr)

define void @g_hod(ptr %buf) {
  %a = ptrtoint ptr %buf to i64
  %b = xor i64 %a, -1
  %c = add i64 %a, 2
  %d = add i64 %a, 3
  %e = add i64 %a, 4
  %f = add i64 %a, 5
  call void @touch(i64 %b)
  %s1 = add i64 %b, %c
  %s2 = add i64 %s1, %d
  %s3 = add i64 %s2, %e
  %s4 = add i64 %s3, %f
  call void @touch(i64 %s4)
  call void @llvm.eh.sjlj.longjmp(ptr %buf)
  unreachable
}
";

/// `llc`'s side: `keep_all(f, g, out)` puts a value in each of `rbx`,
/// `r12`..`r15` (WP-146's `tests/text_size.rs`), calls `f(g)` and
/// stores the five registers to `out` after the return; `g_llc(buf)`
/// overwrites all five and jumps.
const SAVE_ALL_LLC_SIDE: &str = r#"
declare void @llvm.eh.sjlj.longjmp(ptr)

define void @keep_all(ptr %f, ptr %g, ptr %out) {
  call void asm sideeffect "movq $$0x1111, %rbx\0Amovq $$0x2222, %r12\0Amovq $$0x3333, %r13\0Amovq $$0x4444, %r14\0Amovq $$0x5555, %r15", "~{rbx},~{r12},~{r13},~{r14},~{r15}"()
  call void %f(ptr %g)
  call void asm sideeffect "movq %rbx, 0($0)\0Amovq %r12, 8($0)\0Amovq %r13, 16($0)\0Amovq %r14, 24($0)\0Amovq %r15, 32($0)", "r,~{rbx},~{r12},~{r13},~{r14},~{r15},~{memory}"(ptr %out)
  ret void
}

define void @g_llc(ptr %buf) {
  call void asm sideeffect "movq $$-1, %rbx\0Amovq $$-2, %r12\0Amovq $$-3, %r13\0Amovq $$-4, %r14\0Amovq $$-5, %r15", "~{rbx},~{r12},~{r13},~{r14},~{r15}"()
  call void @llvm.eh.sjlj.longjmp(ptr %buf)
  unreachable
}
"#;

const SAVE_ALL_DRIVER: &str = r#"
#include <stdio.h>
#include <stdint.h>
typedef uint64_t u;
extern int resumed;
void f_sj(void *);
void g_hod(void *);
void g_llc(void *);
void keep_all(void (*)(void *), void *, u *);
__attribute__((noinline)) void touch(u v) { __asm__ volatile("" : : "r"(v)); }
int main(void) {
  void *gs[2] = {(void *)g_hod, (void *)g_llc};
  for (int i = 0; i < 2; i++) {
    u out[5] = {0};
    keep_all(f_sj, gs[i], out);
    printf("%s: resumed %d regs %llx %llx %llx %llx %llx\n", i ? "llc" : "hod", resumed,
           (unsigned long long)out[0], (unsigned long long)out[1], (unsigned long long)out[2],
           (unsigned long long)out[3], (unsigned long long)out[4]);
  }
  return 0;
}
"#;

/// §5.6.15 item 1, §5.6.16 (2) with WP-146's save set: a `setjmp`
/// function saves every callee-saved register (`FrameState::save_all`)
/// even when its body holds none, because the resume point is entered
/// with the jumper's registers. `llc` code holding values in `rbx`,
/// `r12`..`r15` calls hod's `F`; `F` sets its buffer and calls `G` (hod's,
/// holding five values in callee-saved registers, or `llc`'s, writing
/// all five), which jumps back; `F` returns, and the caller's five values
/// are intact. `F` plain and stressed, every linker.
#[test]
fn a_setjmp_function_restores_every_callee_saved_register() {
    let scratch = hod_testutil::Scratch::new("x64-sjlj-save-all");
    let jumper = hod_object(&scratch, "g-hod", SAVE_ALL_HOD_JUMPER, Stress::default());
    let text = format!("{}{SAVE_ALL_LLC_SIDE}", ll_header());
    let Some(llc_side) = float_gen::llc(&scratch, 22, "save-all-llc", &text) else { return };
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    let driver = toolchain.compile_c(&scratch, "save-all-driver", SAVE_ALL_DRIVER, &[]);
    let want = "hod: resumed 1 regs 1111 2222 3333 4444 5555\n\
                llc: resumed 2 regs 1111 2222 3333 4444 5555\n";
    let mut runs = 0;
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let setter = hod_object(&scratch, &format!("f-{mode}"), SAVE_ALL_SETTER, stress);
        for linker in &toolchain.linkers {
            let program = scratch.join(format!("save-all-{mode}-{}", linker.name));
            let ran = toolchain.link_and_run(
                linker,
                &[],
                &[&driver, &setter, &jumper, &llc_side],
                &program,
            );
            assert!(
                ran.success(),
                "{mode} [{}]: {:?}\n{}",
                linker.name,
                ran.status,
                ran.stderr_text()
            );
            assert_eq!(ran.stdout_text(), want, "{mode} [{}]", linker.name);
            runs += 1;
        }
    }
    eprintln!("sjlj save set: {runs} programs");
}

// ---- 3. constant masks across `phi`, calls and returns ----

/// The mask constant of `n` lanes whose lane `i` is bit `i` of `bits`
/// (without its type).
fn mask_const(n: u32, bits: u64) -> String {
    let lanes: Vec<String> = (0..n).map(|i| format!("i1 {}", (bits >> i) & 1)).collect();
    format!("<{}>", lanes.join(", "))
}

/// §5.6.16 (3): a constant mask other than zero as a `phi` incoming (of
/// every lane count), a call argument and a returned value (of the
/// counts a call takes: 1, 2, 4, 8, 16), all-ones, one lane, alternating
/// and random patterns.
#[test]
fn constant_masks_agree() {
    let mut suite = Suite::default();
    let mut rng = float_gen::Rng(147);
    for n in [1u32, 2, 3, 4, 7, 8, 16, 17, 32, 33, 63, 64] {
        let all = float_gen::mask(n);
        let patterns = [all, 1, 1 << (n - 1), 0x5555_5555_5555_5555 & all, rng.next() & all];
        for (k, &bits) in patterns.iter().enumerate() {
            let m = mask_const(n, bits);
            let ty = format!("<{n} x i1>");
            // The incoming constant or the argument's bits, by bit 63 of
            // the second operand.
            let f = suite.add(
                &format!("mphi{n}_{k}"),
                &[T::I(n), I64],
                T::I(n),
                &format!(
                    "  %v = bitcast i{n} %a0 to {ty}\n  %c = icmp slt i64 %a1, 0\n  \
                     br i1 %c, label %t, label %e\nt:\n  br label %j\ne:\n  br label %j\nj:\n  \
                     %m = phi {ty} [ {m}, %t ], [ %v, %e ]\n  %r = bitcast {ty} %m to i{n}\n"
                ),
                Check::Interp,
            );
            for a1 in [0, u64::MAX] {
                suite.case(f, &[rng.next(), a1]);
            }
            if ![1, 2, 4, 8, 16].contains(&n) {
                continue;
            }
            suite.declare(&format!(
                "define i{n} @mtake{n}_{k}({ty} %m) {{\n  %b = bitcast {ty} %m to i{n}\n  ret i{n} %b\n}}\n\
                 define {ty} @mgive{n}_{k}() {{\n  ret {ty} {m}\n}}"
            ));
            let f = suite.add(
                &format!("mcall{n}_{k}"),
                &[I64],
                T::I(n),
                &format!(
                    "  %x = call i{n} @mtake{n}_{k}({ty} {m})\n  %g = call {ty} @mgive{n}_{k}()\n  \
                     %y = bitcast {ty} %g to i{n}\n  %q = xor i{n} %x, %y\n  %r = xor i{n} %q, %x\n"
                ),
                Check::Interp,
            );
            suite.case(f, &[0]);
        }
    }
    check("const-masks", &suite, &MAJORS);
}

// ---- 4. `fp128` lanes ----

/// §5.6.16 (4): a lane operation on a vector of `fp128` stays refused as
/// `x64:vector-wide-element` (no corpus module has one; every lane
/// operation would be a libcall per lane, and the vector's call ABI is
/// another package).
#[test]
fn fp128_lanes_are_refused_by_name() {
    let wide = hod_x64::vector::WIDE_ELEMENT;
    for body in [
        "  %v = load <2 x fp128>, ptr %p, align 16\n  %r = fadd <2 x fp128> %v, %v\n  \
         store <2 x fp128> %r, ptr %p, align 16\n",
        "  %v = load <2 x fp128>, ptr %p, align 16\n  %e = extractelement <2 x fp128> %v, i64 1\n  \
         store fp128 %e, ptr %p, align 16\n",
    ] {
        refused(&format!("define void @f(ptr %p) {{\n{body}  ret void\n}}\n"), wide);
    }
}

// ---- 5. `fptrunc x86_fp80` to `bfloat` ----

/// The body that builds `%x`, an `x86_fp80`, from the words `%a0` (the
/// significand) and `%a1` (the sign and exponent).
const X87_OPERAND: &str = "  %xe = zext i64 %a1 to i80\n  %xh = shl i80 %xe, 64\n  \
     %xm = zext i64 %a0 to i80\n  %xb = or i80 %xh, %xm\n  %x = bitcast i80 %xb to x86_fp80\n";

/// `x86_fp80` operands as (significand, sign and exponent): zeros, the
/// smallest and largest denormals, a pseudo-denormal, normals around
/// `bfloat`'s range and its rounding boundaries (ties to even), the
/// infinities, quiet and signalling NaNs, the invalid encodings, and
/// random values near `bfloat`'s exponents.
fn x87_operands() -> Vec<[u64; 2]> {
    const J: u64 = 1 << 63;
    let mut v = vec![
        [0, 0],
        [0, 0x8000],
        [1, 0],
        [J - 1, 0x8000],
        [J | 5, 0],
        [J, 0x3fff],
        [J, 0xbfff],
        [J | 0x0080_0000_0000_0000, 0x3fff],
        [J | 0x0080_0000_0000_0001, 0x3fff],
        [J | 0x0180_0000_0000_0000, 0x3fff],
        [J | 0x00ff_ffff_ffff_ffff, 0x3fff],
        [u64::MAX, 0x407e],
        [J, 0x407f],
        [J, 0x3f81],
        [J, 0x3f80],
        [J, 0x3f78],
        [J, 0x3f77],
        [J, 0x3f76],
        [J | 0x00c0_0000_0000_0000, 0x3f77],
        [J, 0x0001],
        [J, 0x7ffe],
        [J, 0x7fff],
        [J, 0xffff],
        [0xc000_0000_0000_0000, 0x7fff],
        [0xc123_4567_89ab_cdef, 0xffff],
        [J | 1, 0x7fff],
        [J | 0x1234_0000_0000_0000, 0xffff],
        [0x4000_0000_0000_0000, 0x3fff],
        [0, 0x7fff],
        [0, 0x3fff],
    ];
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for _ in 0..400 {
        state =
            state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let m = state | J;
        let e = (0x3f70 + (state >> 40) % 0x120) | (state >> 20 & 1) << 15;
        v.push([m, e]);
    }
    v
}

/// §5.6.16 (5): `fptrunc x86_fp80` to `bfloat` calls `__truncxfbf2`, as
/// `llc` 22 does (libgcc 13 and later have it; `llc` 19 cannot select the
/// conversion, so it is compared from 22 on). The `bfloat`'s bits come
/// back in `xmm0`; the result is checked through a `bitcast` to `i16`
/// and also through `fpext` to `float` (the value used by hod again).
#[test]
fn x87_to_bfloat_agrees_with_llc_22() {
    let mut suite = Suite::default();
    let bits = suite.add(
        "xf_to_bf",
        &[I64, I64],
        T::I(16),
        &format!(
            "{X87_OPERAND}  %t = fptrunc x86_fp80 %x to bfloat\n  %r = bitcast bfloat %t to i16\n"
        ),
        Check::Llc,
    );
    let wide = suite.add(
        "xf_to_bf_to_f",
        &[I64, I64],
        T::F,
        &format!(
            "{X87_OPERAND}  %t = fptrunc x86_fp80 %x to bfloat\n  %r = fpext bfloat %t to float\n"
        ),
        Check::Llc,
    );
    for f in [bits, wide] {
        suite.funcs[f].since = 22;
        for v in x87_operands() {
            suite.case(f, &v);
        }
    }
    check("xf-to-bf", &suite, &MAJORS);
}

// ---- 9. `llvm.is.fpclass` of a pseudo-denormal ----

/// §5.6.12: a pseudo-denormal (exponent 0, integer bit set) is a
/// subnormal for `llvm.is.fpclass`, as `fxam` reports it, with every
/// one of the 1024 masks. `llc` 19 and 22 put it in no class (false for
/// every single class) but answer true for some unions by their
/// expansion (19: `0x7`, `0xf3`, `0x203`; 22: `0x30c`), which no class
/// gives; hod is checked against the rule, not against them.
#[test]
fn is_fpclass_of_a_pseudo_denormal_is_subnormal() {
    let mut text = String::from("declare i1 @llvm.is.fpclass.f80(x86_fp80, i32)\n");
    let mut c = String::from("#include <stdio.h>\n#include <stdint.h>\n");
    let mut table = String::new();
    for mask in 0..1024 {
        write!(
            text,
            "define i32 @c{mask}(i64 %a0, i64 %a1) {{\n{X87_OPERAND}  \
             %t = call i1 @llvm.is.fpclass.f80(x86_fp80 %x, i32 {mask})\n  \
             %r = zext i1 %t to i32\n  ret i32 %r\n}}\n"
        )
        .unwrap();
        writeln!(c, "int c{mask}(uint64_t, uint64_t);").unwrap();
        write!(table, "c{mask},").unwrap();
    }
    write!(
        c,
        "static int (*const fs[])(uint64_t, uint64_t) = {{{table}}};\n\
         static const uint64_t vs[4][2] = {{{{1ull << 63, 0}}, {{(1ull << 63) | 1, 0}}, \
         {{0xc000000000000000ull, 0x8000}}, {{~0ull, 0x8000}}}};\n\
         int main(void) {{\n  int wrong = 0;\n  for (int k = 0; k < 1024; k++)\n    \
         for (int i = 0; i < 4; i++) {{\n      int want = (k >> (vs[i][1] ? 4 : 7)) & 1;\n      \
         if (fs[k](vs[i][0], vs[i][1]) != want) {{ printf(\"mask %#x value %d\\n\", k, i); \
         wrong++; }}\n    }}\n  printf(\"%d wrong\\n\", wrong);\n  return wrong != 0;\n}}\n"
    )
    .unwrap();
    let scratch = hod_testutil::Scratch::new("x64-fpclass-pd");
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    let driver = toolchain.compile_c(&scratch, "fpclass-driver", &c, &[]);
    for (name, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let object = hod_object(&scratch, &format!("fpclass-{name}"), &text, stress);
        let linker = &toolchain.linkers[0];
        let program = scratch.join(format!("fpclass-{name}"));
        let ran = toolchain.link_and_run(linker, &[], &[&driver, &object], &program);
        assert!(ran.success(), "{name}: {}{}", ran.stdout_text(), ran.stderr_text());
        assert_eq!(ran.stdout_text(), "0 wrong\n", "{name}");
    }
}
