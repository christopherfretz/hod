//! x86-64's `musttail` stack-argument rule at the facade (WP-148,
//! `spec/abi-x64.md` §8.5, `spec/codegen.md` §7.1 "The `musttail` hook").
//!
//! `classify` learns both stack-argument areas of a `musttail` call from
//! `hod_cg::abi::TargetStackArgs::per_function` with
//! `hod_x64::call::target_of`: the caller's own signature and the call,
//! each assigned by `X64::assign` with the features of the function that
//! makes the call. A call whose arguments fit the caller's area compiles;
//! one that needs more is `musttail:stack-args-grow`; one the rule cannot
//! size stays `musttail:stack-args-unknown`.
//!
//! The generated cases are the thunks of `crates/hod_a64/tests/
//! musttail_varargs.rs` retargeted: `t_k(named...)` or `t_k(named...,
//! ...)` tail-calls `c_k` of the same type with its own parameters,
//! permuted among those of one type and some computed. The parameters
//! are of every class the SysV ABI has (`i8 signext`, `i16 zeroext`,
//! `i32`, `i64`, `ptr`, `i128`, `float`, `double`, `x86_fp80`, `fp128`,
//! `<2 x double>`, `<4 x i32>`, `byval` structures aligned to 8 and 16,
//! an `sret` result), enough of them to put some on the stack; some
//! thunks first call functions that set every argument register and
//! `al`. A variadic thunk's caller passes forwarded arguments of its own
//! (in registers and on the stack) that the C callee reads by `va_arg`.
//! A third of the variadic thunks list variable arguments before the
//! `...`: those are judged only (`spec/abi-x64.md` §8.5).
//!
//! Every case is compiled through the facade on its own, and the outcome
//! is compared with a model of the rule written here from
//! `spec/abi-x64.md` §3, §4 and §6. The ones that compile are built into
//! one object by the facade, one by `hod_x64` with every stress option,
//! and one by `llc -O0 -fast-isel=0 -relocation-model=pic` 19 and 22,
//! each linked with the same C callers and callees (`gcc` for x86-64)
//! under GNU ld, lld and mold and run under the emulator. The callee
//! checks every parameter and forwarded argument against the value the
//! caller passed; the caller checks canaries in its frame and values in
//! callee-saved registers across the call.

#![allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::similar_names,
    reason = "generated values of small ranges, named as the C side names them"
)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod::{CompileError, Options, OutputKind, RelocModel, Report, Settings};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with};
use linking::{Toolchain, exit_code};

fn header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// A small deterministic generator (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The classes of parameters (and of variable arguments).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    I8,
    I16,
    I32,
    I64,
    Ptr,
    I128,
    F32,
    F64,
    F80,
    F128,
    V2D,
    V4I,
    S24,
    S32,
}

const TYS: [Ty; 14] = [
    Ty::I8,
    Ty::I16,
    Ty::I32,
    Ty::I64,
    Ty::Ptr,
    Ty::I128,
    Ty::F32,
    Ty::F64,
    Ty::F80,
    Ty::F128,
    Ty::V2D,
    Ty::V4I,
    Ty::S24,
    Ty::S32,
];

/// The forwarded arguments a variadic thunk's caller passes.
const FORWARDED: [Ty; 6] = [Ty::I64, Ty::F64, Ty::F80, Ty::I128, Ty::S24, Ty::V2D];

/// The variable arguments listed before a forwarded `...`.
const EXTRAS: [Ty; 9] =
    [Ty::I8, Ty::I32, Ty::I64, Ty::I128, Ty::F32, Ty::F64, Ty::F80, Ty::F128, Ty::V2D];

impl Ty {
    /// The parameter as the IR declares it, attributes included.
    fn ir(self) -> &'static str {
        match self {
            Ty::I8 => "i8 signext",
            Ty::I16 => "i16 zeroext",
            Ty::S24 => "ptr byval(%s24) align 8",
            Ty::S32 => "ptr byval(%s32) align 16",
            other => other.bare(),
        }
    }

    /// The type in a function type.
    fn bare(self) -> &'static str {
        match self {
            Ty::I8 => "i8",
            Ty::I16 => "i16",
            Ty::I32 => "i32",
            Ty::I64 => "i64",
            Ty::Ptr | Ty::S24 | Ty::S32 => "ptr",
            Ty::I128 => "i128",
            Ty::F32 => "float",
            Ty::F64 => "double",
            Ty::F80 => "x86_fp80",
            Ty::F128 => "fp128",
            Ty::V2D => "<2 x double>",
            Ty::V4I => "<4 x i32>",
        }
    }

    fn c(self) -> &'static str {
        match self {
            Ty::I8 => "signed char",
            Ty::I16 => "unsigned short",
            Ty::I32 => "unsigned",
            Ty::I64 => "unsigned long",
            Ty::Ptr => "char *",
            Ty::I128 => "unsigned __int128",
            Ty::F32 => "float",
            Ty::F64 => "double",
            Ty::F80 => "long double",
            Ty::F128 => "__float128",
            Ty::V2D => "v2d",
            Ty::V4I => "v4i",
            Ty::S24 => "struct s24",
            Ty::S32 => "struct s32",
        }
    }

    /// A value of the type as a C expression.
    fn c_value(self, rng: &mut Rng) -> String {
        let r = rng.next();
        let real = |rng: &mut Rng| (rng.below(200_000) as f64 - 100_000.0) / 64.0;
        match self {
            Ty::I8 => format!("((signed char){})", r as u8 as i8),
            Ty::I16 => format!("((unsigned short){})", r as u16),
            Ty::I32 => format!("{}u", r as u32),
            Ty::I64 => format!("{r:#x}UL"),
            Ty::Ptr => format!("((char *){r:#x}UL)"),
            Ty::I128 => format!("((unsigned __int128){:#x}UL << 64 | {r:#x}UL)", rng.next()),
            Ty::F32 => format!("{:?}f", (rng.below(2000) as f32 - 1000.0) / 8.0),
            Ty::F64 => format!("({:?})", real(rng)),
            Ty::F80 => format!("((long double){:?})", real(rng)),
            Ty::F128 => format!("((__float128){:?})", real(rng)),
            Ty::V2D => format!("((v2d){{{:?}, {:?}}})", real(rng), real(rng)),
            Ty::V4I => format!(
                "((v4i){{{}, {}, {}, {}}})",
                r as u32 as i32,
                (r >> 32) as u32 as i32,
                rng.next() as u32 as i32,
                rng.next() as u32 as i32
            ),
            Ty::S24 => format!("((struct s24){{{r:#x}UL, {:#x}UL, {:#x}UL}})", rng.next(), !r),
            Ty::S32 => format!("((struct s32){{(long double){:?}, {r:#x}UL}})", real(rng)),
        }
    }

    /// A constant of the type as an IR operand (variable arguments).
    fn ir_value(self, rng: &mut Rng) -> String {
        let r = rng.below(1000);
        match self {
            Ty::I8 => format!("i8 {}", r as u8 as i8),
            Ty::I32 => format!("i32 {r}"),
            Ty::I64 => format!("i64 {r}"),
            Ty::I128 => format!("i128 {r}"),
            Ty::F32 => "float 1.5".into(),
            Ty::F64 => format!("double {r}.0"),
            Ty::F80 => "x86_fp80 0xK3FFF8000000000000000".into(),
            Ty::F128 => "fp128 0xL00000000000000003FFF000000000000".into(),
            Ty::V2D => format!("<2 x double> <double {r}.0, double 2.0>"),
            _ => unreachable!("not a variable argument here"),
        }
    }
}

// ---- The model of the rule (`spec/abi-x64.md` §3.1, §3.2, §4.2, §6.2) ----

/// The SysV assignment state: next general register, next vector
/// register, next stack offset.
#[derive(Default)]
struct Sysv {
    g: u32,
    v: u32,
    o: u64,
}

impl Sysv {
    fn slot(&mut self, align: u64, size: u64) {
        self.o = self.o.next_multiple_of(align) + size;
    }

    fn place(&mut self, ty: Ty) {
        match ty {
            Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::Ptr => {
                if self.g < 6 {
                    self.g += 1;
                } else {
                    self.slot(8, 8);
                }
            }
            // §4.2: never split; on the stack, the register stays.
            Ty::I128 => {
                if self.g <= 4 {
                    self.g += 2;
                } else {
                    self.slot(16, 16);
                }
            }
            Ty::F32 | Ty::F64 => {
                if self.v < 8 {
                    self.v += 1;
                } else {
                    self.slot(8, 8);
                }
            }
            Ty::F128 | Ty::V2D | Ty::V4I => {
                if self.v < 8 {
                    self.v += 1;
                } else {
                    self.slot(16, 16);
                }
            }
            Ty::F80 => self.slot(16, 16),
            Ty::S24 => self.slot(8, 24),
            Ty::S32 => self.slot(16, 32),
        }
    }
}

/// The `stack_size` of a signature: an `sret` pointer first, if any.
fn model_stack(sret: bool, tys: impl IntoIterator<Item = Ty>) -> u64 {
    let mut state = Sysv::default();
    if sret {
        state.place(Ty::Ptr);
    }
    for ty in tys {
        state.place(ty);
    }
    state.o
}

/// Where a parameter's value comes from.
#[derive(Clone, Copy)]
enum Source {
    /// Parameter `j` of the thunk.
    Param(usize),
    /// `add i64 %pj, n`.
    Add(usize, u64),
}

struct Case {
    k: usize,
    sret: bool,
    variadic: bool,
    params: Vec<Ty>,
    /// The C value the caller passes for each parameter.
    values: Vec<String>,
    /// What the thunk passes on as each parameter.
    sources: Vec<Source>,
    /// Variable arguments before the `...` (variadic thunks only).
    extras: Vec<(Ty, String)>,
    /// Forwarded arguments the caller passes (variadic thunks only).
    forwarded: Vec<(Ty, String)>,
    clobber: bool,
}

/// What the rule and the code generator answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    Compiles,
    /// `musttail:stack-args-grow`.
    Grows,
    /// The call fits, and `hod_x64` refuses its variable arguments
    /// (`x64:musttail-variable-arguments`, §8.5).
    VariableArguments,
}

impl Case {
    /// The model: both areas, compared as `classify` compares them.
    fn predicted(&self) -> Outcome {
        let have = model_stack(self.sret, self.params.iter().copied());
        let need = model_stack(
            self.sret,
            self.params.iter().copied().chain(self.extras.iter().map(|e| e.0)),
        );
        if need > have {
            Outcome::Grows
        } else if self.extras.is_empty() {
            Outcome::Compiles
        } else {
            Outcome::VariableArguments
        }
    }

    /// Whether `llc` 19 builds the thunk otherwise than the psABI says:
    /// - it places the parameters otherwise (`spec/abi-x64.md` §4.2
    ///   "LLVM 19"): an `i128` that goes to the stack with one general
    ///   register left, then a general parameter (LLVM 22 and the psABI
    ///   give it that register, LLVM 19 the stack);
    /// - a `byval` parameter: `llc` 19 stages the copies of a `musttail`
    ///   call's `byval` arguments at `rsp` of the thunk, over its return
    ///   address (the program crashes; `llc` 22 does not).
    fn llvm19_differs(&self) -> bool {
        if self.params.iter().any(|&ty| matches!(ty, Ty::S24 | Ty::S32)) {
            return true;
        }
        let mut state = Sysv::default();
        let mut stranded = false;
        let all = self.sret.then_some(Ty::Ptr).into_iter().chain(self.params.iter().copied());
        for ty in all {
            match ty {
                Ty::I128 if state.g == 5 => stranded = true,
                Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::Ptr if stranded => return true,
                _ => {}
            }
            state.place(ty);
        }
        false
    }

    /// Whether the caller's own named arguments use the stack.
    fn named_on_stack(&self) -> bool {
        model_stack(self.sret, self.params.iter().copied()) > 0
    }
}

fn random_case(rng: &mut Rng, k: usize) -> Case {
    let count = 1 + rng.below(18) as usize;
    let params: Vec<Ty> = (0..count).map(|_| TYS[rng.below(TYS.len() as u64) as usize]).collect();
    let values: Vec<String> = params.iter().map(|ty| ty.c_value(rng)).collect();
    let mut sources = Vec::new();
    for (i, &ty) in params.iter().enumerate() {
        let same: Vec<usize> = (0..count).filter(|&j| params[j] == ty).collect();
        let j = if rng.below(2) == 0 { i } else { same[rng.below(same.len() as u64) as usize] };
        if ty == Ty::I64 && rng.below(3) == 0 {
            sources.push(Source::Add(j, rng.next() >> 20));
        } else {
            sources.push(Source::Param(j));
        }
    }
    let variadic = rng.below(3) != 0;
    let mut extras = Vec::new();
    let mut forwarded = Vec::new();
    if variadic {
        if rng.below(3) == 0 {
            for _ in 0..=rng.below(8) {
                let ty = EXTRAS[rng.below(EXTRAS.len() as u64) as usize];
                extras.push((ty, ty.ir_value(rng)));
            }
        }
        for _ in 0..rng.below(14) {
            let ty = FORWARDED[rng.below(FORWARDED.len() as u64) as usize];
            forwarded.push((ty, ty.c_value(rng)));
        }
    }
    Case {
        k,
        sret: rng.below(4) == 0,
        variadic,
        params,
        values,
        sources,
        extras,
        forwarded,
        clobber: rng.below(3) == 0,
    }
}

/// The thunks and what they declare.
fn thunks(cases: &[&Case]) -> String {
    let mut out = String::from(
        "%s24 = type { i64, i64, i64 }\n%s32 = type { x86_fp80, i64 }\n\
         declare void @clobber(i64, i64, i64, i64, i64, i64, double, double, double, double, \
         double, double, double, double)\n\
         declare void @zero_al(i32, ...)\n",
    );
    for case in cases {
        let k = case.k;
        let mut decl: Vec<String> = Vec::new();
        let mut def: Vec<String> = Vec::new();
        let mut fn_ty: Vec<&str> = Vec::new();
        let mut args: Vec<String> = Vec::new();
        if case.sret {
            decl.push("ptr sret(%s24) align 8".into());
            def.push("ptr sret(%s24) align 8 %ps".into());
            fn_ty.push("ptr");
            args.push("ptr sret(%s24) align 8 %ps".into());
        }
        let mut body = String::new();
        for (i, &ty) in case.params.iter().enumerate() {
            decl.push(ty.ir().into());
            def.push(format!("{} %p{i}", ty.ir()));
            fn_ty.push(ty.bare());
            match case.sources[i] {
                Source::Param(j) => args.push(format!("{} %p{j}", ty.ir())),
                Source::Add(j, n) => {
                    writeln!(body, "  %a{i} = add i64 %p{j}, {n}").unwrap();
                    args.push(format!("i64 %a{i}"));
                }
            }
        }
        for (_, operand) in &case.extras {
            args.push(operand.clone());
        }
        let dots = if case.variadic { ", ..." } else { "" };
        let ret = if case.sret { "void" } else { "i64" };
        writeln!(out, "declare {ret} @c_{k}({}{dots})", decl.join(", ")).unwrap();
        writeln!(out, "define {ret} @t_{k}({}{dots}) noinline {{", def.join(", ")).unwrap();
        if case.clobber {
            out.push_str(
                "  call void @clobber(i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, \
                 double -9.0, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0, \
                 double -9.0, double -9.0)\n  call void (i32, ...) @zero_al(i32 0)\n",
            );
        }
        out.push_str(&body);
        let call = format!("{ret} ({}{dots}) @c_{k}({}{dots})", fn_ty.join(", "), args.join(", "));
        if case.sret {
            writeln!(out, "  musttail call {call}\n  ret void\n}}").unwrap();
        } else {
            writeln!(out, "  %r = musttail call {call}\n  ret i64 %r\n}}").unwrap();
        }
    }
    out
}

/// `if` the value `p` (of `ty`) differs from `want`.
fn c_check(ty: Ty, p: &str, want: &str, tag: &str) -> String {
    let differs = match ty {
        Ty::F80 => format!("memcmp(&{p}, &w_, 10)"),
        Ty::S32 => format!("memcmp(&{p}.x, &w_.x, 10) || {p}.y != w_.y"),
        _ => format!("memcmp(&{p}, &w_, sizeof w_)"),
    };
    format!(
        "  {{ {} w_ = {want}; if ({differs}) {{ printf(\" bad-{tag}\"); ok = 0; }} }}\n",
        ty.c()
    )
}

/// The C side: `clobber`, `zero_al`, every `c_k` and `run_k`, `main`.
fn c_side(cases: &[&Case]) -> String {
    let mut out = String::from(
        "#include <stdarg.h>\n#include <stdio.h>\n#include <string.h>\n\
         typedef double v2d __attribute__((vector_size(16)));\n\
         typedef int v4i __attribute__((vector_size(16)));\n\
         struct s24 { unsigned long a, b, c; };\n\
         struct s32 { long double x; unsigned long y; };\n\
         static long opaque(long x) { __asm__ volatile(\"\" : \"+r\"(x)); return x; }\n\
         static int bad;\n\
         __attribute__((noinline)) void clobber(long a, long b, long c, long d, long e, long f, \
         double i, double j, double k, double l, double m, double n, double o, double p) \
         { __asm__ volatile(\"\" ::: \"memory\"); }\n\
         __attribute__((noinline)) void zero_al(int a, ...) { __asm__ volatile(\"\" ::: \"memory\"); }\n",
    );
    for case in cases {
        let k = case.k;
        let params: Vec<String> =
            case.params.iter().enumerate().map(|(i, ty)| format!("{} p{i}", ty.c())).collect();
        let dots = if case.variadic { ", ..." } else { "" };
        let ret = if case.sret { "struct s24" } else { "long" };
        writeln!(out, "{ret} t_{k}({}{dots});", params.join(", ")).unwrap();
        writeln!(out, "{ret} c_{k}({}{dots}) {{\n  int ok = 1;", params.join(", ")).unwrap();
        for (i, &ty) in case.params.iter().enumerate() {
            let want = match case.sources[i] {
                Source::Param(j) => case.values[j].clone(),
                Source::Add(j, n) => format!("({} + {n:#x}UL)", case.values[j]),
            };
            out.push_str(&c_check(ty, &format!("p{i}"), &want, &format!("p{i}")));
        }
        if case.variadic {
            writeln!(out, "  va_list ap;\n  va_start(ap, p{});", case.params.len() - 1).unwrap();
            for (f, (ty, value)) in case.forwarded.iter().enumerate() {
                writeln!(out, "  {{ {} v_ = va_arg(ap, {});", ty.c(), ty.c()).unwrap();
                out.push_str(&c_check(*ty, "v_", value, &format!("f{f}")));
                out.push_str("  }\n");
            }
            out.push_str("  va_end(ap);\n");
        }
        writeln!(out, "  printf(\"{k}:%s\", ok ? \" ok\" : \"\");\n  if (!ok) bad = 1;").unwrap();
        if case.sret {
            writeln!(out, "  return (struct s24){{ {k}, 2 * {k}, 3 * {k} }};\n}}").unwrap();
        } else {
            writeln!(out, "  return {};\n}}", k * 7 + 1).unwrap();
        }

        // The caller: canaries in its frame, a value across the call.
        let mut args = case.values.clone();
        args.extend(case.forwarded.iter().map(|(_, value)| value.clone()));
        let (call, result) = if case.sret {
            (
                format!("struct s24 r = t_{k}({});", args.join(", ")),
                format!(
                    "if (r.a != {k} || r.b != 2 * {k} || r.c != 3 * {k}) {{ printf(\" RET\"); bad = 1; }}\n\
                     \x20 printf(\" -> s\\n\");"
                ),
            )
        } else {
            (
                format!("long r = t_{k}({});", args.join(", ")),
                "printf(\" -> %lx\\n\", r);".to_string(),
            )
        };
        writeln!(
            out,
            "__attribute__((noinline)) static void run_{k}(void) {{\n\
             \x20 volatile unsigned long canary[4] = {{ 0xc0ffee{k:x}, 1, 2, 3 }};\n\
             \x20 long keep = opaque({k}) * 3 + 1;\n\
             \x20 {call}\n\
             \x20 if (canary[0] != 0xc0ffee{k:x} || canary[1] != 1 || canary[2] != 2 || canary[3] != 3)\n\
             \x20   {{ printf(\" CANARY\"); bad = 1; }}\n\
             \x20 if (keep != {k} * 3 + 1) {{ printf(\" SAVED\"); bad = 1; }}\n\
             \x20 {result}\n}}"
        )
        .unwrap();
    }
    out.push_str("int main(void) {\n");
    for case in cases {
        writeln!(out, "  run_{}();", case.k).unwrap();
    }
    out.push_str("  printf(\"done\\n\");\n  return bad;\n}\n");
    out
}

fn options() -> Options {
    Options {
        output_kind: OutputKind::Object,
        relocation_model: Some(RelocModel::Pic),
        ..Options::default()
    }
}

/// The facade's answer for `text` (a module without its header).
fn facade(text: &str) -> Result<Vec<u8>, CompileError> {
    let text = format!("{}{text}", header());
    hod::compile_with(text.as_bytes(), &options(), &Settings::default(), &mut Report::default())
}

/// The name a refusal gives, `None` when the module compiles.
fn refusal(text: &str) -> Option<String> {
    match facade(text) {
        Ok(_) => None,
        Err(CompileError::Unsupported(unsupported)) => Some(unsupported.construct.name().into()),
        Err(other) => panic!("{other}\n{text}"),
    }
}

fn judged(case: &Case) -> Outcome {
    match refusal(&thunks(&[case])).as_deref() {
        None => Outcome::Compiles,
        Some("musttail:stack-args-grow") => Outcome::Grows,
        Some("x64:musttail-variable-arguments") => Outcome::VariableArguments,
        Some(other) => panic!("case {}: {other}\n{}", case.k, thunks(&[case])),
    }
}

/// `hod_x64` below the facade with every stress option.
fn stressed(text: &str) -> Vec<u8> {
    let text = format!("{}{text}", header());
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, read| {
        bodies.push(read.body.expect("every body is read").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let options = hod_cg::Options {
        reloc_model: hod_cg::RelocModel::Pic,
        stress: hod_cg::Stress::ALL,
        ..hod_cg::Options::default()
    };
    hod_x64::compile_to_object(&module, &bodies, &options).unwrap_or_else(|e| panic!("{e}"))
}

/// `llc` of `major`, `-O0 -fast-isel=0 -relocation-model=pic`, if
/// installed.
fn llc_object(dir: &Scratch, major: u32, text: &str) -> Option<PathBuf> {
    let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if !llc.is_file() {
        assert!(!hod_testutil::tools_required(), "llc {major} is required");
        eprintln!("note: llc {major} not found");
        return None;
    }
    let input = dir.write(&format!("t-llc{major}.ll"), format!("{}{text}", header()));
    let output = dir.join(format!("t-llc{major}.o"));
    let mut command = Command::new(llc);
    command.args(["-O0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"]);
    command.arg("-mtriple=x86_64-unknown-linux-gnu").arg(&input).arg("-o").arg(&output);
    let ran = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(ran.success(), "llc {major}: {}", ran.stderr_text());
    Some(output)
}

/// The bytes of an ELF64 object's executable sections.
fn text_bytes(object: &[u8]) -> u64 {
    let word = |at: usize, n: usize| {
        object[at..at + n].iter().rev().fold(0u64, |acc, &b| acc << 8 | u64::from(b))
    };
    let (shoff, shentsize, shnum) = (word(0x28, 8), word(0x3a, 2), word(0x3c, 2));
    (0..shnum)
        .map(|i| (shoff + i * shentsize) as usize)
        .filter(|&at| word(at + 8, 8) & 0x4 != 0)
        .map(|at| word(at + 0x20, 8))
        .sum()
}

fn run(toolchain: &Toolchain, dir: &Scratch, tag: &str, inputs: &[&Path]) -> Vec<String> {
    let mut outputs = Vec::new();
    for linker in &toolchain.linkers {
        let exe = dir.join(format!("{tag}-{}", linker.name));
        let ran = toolchain.link_and_run(linker, &["-pie"], inputs, &exe);
        assert_eq!(exit_code(&ran), Some(0), "{tag} {}: {}", linker.name, ran.stdout_text());
        outputs.push(ran.stdout_text());
    }
    outputs
}

#[derive(Default, Debug)]
struct Totals {
    compiled: usize,
    grown: usize,
    variable: usize,
    named_on_stack: usize,
    variadic_run: usize,
    runs: usize,
    llvm19_skipped: usize,
    hod_text: u64,
    llc_text: u64,
    classes: [usize; TYS.len()],
}

fn run_seed(seed: u64, count: usize, totals: &mut Totals) {
    let mut rng = Rng(seed);
    let cases: Vec<Case> = (0..count).map(|k| random_case(&mut rng, k)).collect();
    let mut compiled = Vec::new();
    for case in &cases {
        let predicted = case.predicted();
        assert_eq!(judged(case), predicted, "case {}:\n{}", case.k, thunks(&[case]));
        match predicted {
            Outcome::Compiles => {
                for &ty in &case.params {
                    totals.classes[TYS.iter().position(|&t| t == ty).unwrap()] += 1;
                }
                totals.named_on_stack += usize::from(case.named_on_stack());
                totals.variadic_run += usize::from(case.variadic);
                compiled.push(case);
            }
            Outcome::Grows => totals.grown += 1,
            Outcome::VariableArguments => totals.variable += 1,
        }
    }
    totals.compiled += compiled.len();
    let text = thunks(&compiled);
    let object = facade(&text).unwrap_or_else(|e| panic!("{e}"));
    let dir = Scratch::new("hod-musttail-x64");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", &c_side(&compiled), &["-w"]);
    let mut reference: Option<String> = None;
    let mut check = |tag: &str, cases: &[&Case], outputs: Vec<String>| {
        for output in outputs {
            let mut lines = output.lines();
            for case in cases {
                let line = lines.next().unwrap_or_default();
                let want = format!("{}: ok -> ", case.k);
                assert!(
                    line.starts_with(&want),
                    "{tag} case {}: {line}\n{}",
                    case.k,
                    thunks(&[case])
                );
            }
            assert_eq!(lines.next(), Some("done"), "{tag}");
            if cases.len() == compiled.len() {
                match &reference {
                    None => reference = Some(output),
                    Some(want) => assert_eq!(&output, want, "{tag}"),
                }
            }
        }
    };
    let hod_o = dir.write("t-hod.o", &object);
    check("hod", &compiled, run(&toolchain, &dir, "hod", &[&driver, &hod_o]));
    let stress_o = dir.write("t-stress.o", stressed(&text));
    check("stress", &compiled, run(&toolchain, &dir, "stress", &[&driver, &stress_o]));
    totals.runs += 2 * toolchain.linkers.len();
    // LLVM 19 places an `i128` that does not fit as the psABI does not
    // (§4.2 "LLVM 19"): its thunks are run without those cases.
    let compiled19: Vec<&Case> = compiled.iter().copied().filter(|c| !c.llvm19_differs()).collect();
    totals.llvm19_skipped += compiled.len() - compiled19.len();
    for major in LLVM_MAJORS {
        let (cases, text, driver) = if major == 19 {
            let driver19 = toolchain.compile_c(&dir, "driver19", &c_side(&compiled19), &["-w"]);
            (&compiled19, thunks(&compiled19), driver19)
        } else {
            (&compiled, text.clone(), driver.clone())
        };
        if let Some(llc_o) = llc_object(&dir, major, &text) {
            if major == 22 {
                totals.hod_text += text_bytes(&object);
                totals.llc_text += text_bytes(&std::fs::read(&llc_o).unwrap());
            }
            let tag = format!("llc{major}");
            check(&tag, cases, run(&toolchain, &dir, &tag, &[&driver, &llc_o]));
            totals.runs += toolchain.linkers.len();
        }
    }
}

// `spec/abi-x64.md` §8.5, `spec/codegen.md` §7.1: the facade's rule
// agrees with the model on every case, and what compiles runs like
// `llc`'s.
#[test]
fn generated_thunks_follow_the_rule_and_run_like_llc() {
    let mut totals = Totals::default();
    for seed in [0x148_0001, 0x148_0002, 0x148_0003] {
        run_seed(seed, 60, &mut totals);
    }
    eprintln!(
        "{} thunks compile ({} variadic, {} with named stack arguments), {} are \
         musttail:stack-args-grow, {} x64:musttail-variable-arguments; {} runs ({} cases \
         not run by llc 19)",
        totals.compiled,
        totals.variadic_run,
        totals.named_on_stack,
        totals.grown,
        totals.variable,
        totals.runs,
        totals.llvm19_skipped
    );
    eprintln!("parameters run, by class {TYS:?}: {:?}", totals.classes);
    if totals.llc_text > 0 {
        eprintln!(
            "text bytes hod/llc 22: {}/{} = {:.2}",
            totals.hod_text,
            totals.llc_text,
            totals.hod_text as f64 / totals.llc_text as f64
        );
    }
    assert!(
        totals.compiled >= 100
            && totals.named_on_stack >= 30
            && totals.variadic_run >= 40
            && totals.grown >= 5
            && totals.variable >= 5,
        "{totals:?}"
    );
    assert!(totals.classes.iter().all(|&n| n >= 20), "{:?}", totals.classes);
}

/// A `<4 x double>` thunk under `features`, its call `musttail`.
fn wide_vector_thunk(features: &str, callee_features: &str, vectors: usize) -> String {
    let params: Vec<String> = (0..vectors).map(|i| format!("<4 x double> %v{i}")).collect();
    let types = vec!["<4 x double>"; vectors].join(", ");
    format!(
        "declare i64 @c({types}) #1\n\
         define i64 @t({}) #0 {{\n  %r = musttail call i64 @c({})\n  ret i64 %r\n}}\n\
         attributes #0 = {{ {features} }}\nattributes #1 = {{ {callee_features} }}\n",
        params.join(", "),
        params.join(", ")
    )
}

// `spec/abi-x64.md` §1.3, §8.5: the features of the function making the
// call decide both areas. Without AVX a `<4 x double>` is two `xmm`
// pieces (some on the stack here), whatever the callee's features; with
// known AVX it is one `ymm` register (§8.7, WP-203). Under AVX-512
// `assign` refuses a 512-bit vector, so the rule has no answer and the
// call is `musttail:stack-args-unknown`, as is an `x86_fp80` under
// `-x87`.
#[test]
fn the_calling_functions_features_decide() {
    assert_eq!(refusal(&wide_vector_thunk("nounwind", "nounwind", 5)), None);
    assert_eq!(refusal(&wide_vector_thunk("nounwind", "\"target-features\"=\"+avx\"", 5)), None);
    assert_eq!(refusal(&wide_vector_thunk("\"target-features\"=\"+avx\"", "nounwind", 1)), None);
    assert_eq!(refusal(&wide_vector_thunk("\"target-cpu\"=\"haswell\"", "nounwind", 1)), None);
    let zmm = wide_vector_thunk("\"target-features\"=\"+avx512f\"", "nounwind", 1)
        .replace("<4 x double>", "<8 x double>");
    assert_eq!(refusal(&zmm).as_deref(), Some("musttail:stack-args-unknown"));
    let x87 = "declare i64 @c(x86_fp80)\n\
               define i64 @t(x86_fp80 %x) \"target-features\"=\"-x87\" {\n  \
               %r = musttail call i64 @c(x86_fp80 %x)\n  ret i64 %r\n}\n";
    assert_eq!(refusal(x87).as_deref(), Some("musttail:stack-args-unknown"));
    assert_eq!(refusal(&x87.replace(" \"target-features\"=\"-x87\"", "")), None);
}

// `spec/abi-x64.md` §8.5: the model's examples, by hand. Five `i64`s and
// an `i128` put the pair on the stack and leave `r9` (§4.2); a variadic
// call's `i64` extra then fits in `r9`, a second one grows the area.
#[test]
fn the_model_by_hand() {
    assert_eq!(model_stack(false, [Ty::I64; 5].into_iter().chain([Ty::I128])), 16);
    assert_eq!(model_stack(false, [Ty::I64; 5].into_iter().chain([Ty::I128, Ty::I64])), 16);
    assert_eq!(model_stack(true, [Ty::F80, Ty::F64]), 16);
    assert_eq!(model_stack(false, [Ty::I8, Ty::S24, Ty::S32]), 64);
    let five = "i64 %a, i64 %b, i64 %c, i64 %d, i64 %e";
    let thunk = |extras: &str| {
        format!(
            "declare i64 @c(i64, i64, i64, i64, i64, i128, ...)\n\
             define i64 @t({five}, i128 %w, ...) {{\n  \
             %r = musttail call i64 (i64, i64, i64, i64, i64, i128, ...) @c({five}, i128 %w{extras}, ...)\n  \
             ret i64 %r\n}}\n"
        )
    };
    assert_eq!(refusal(&thunk("")), None);
    assert_eq!(refusal(&thunk(", i64 1")).as_deref(), Some("x64:musttail-variable-arguments"));
    assert_eq!(refusal(&thunk(", i64 1, i64 2")).as_deref(), Some("musttail:stack-args-grow"));
}
