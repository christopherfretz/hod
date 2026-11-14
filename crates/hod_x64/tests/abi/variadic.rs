//! Variadic functions (`spec/abi-x64.md` §9) against C code that walks a
//! `va_list` the way clang writes it.
//!
//! In one direction a hod-assembled caller passes variable arguments to a
//! C function that takes them with `va_arg`. In the other an LLVM-compiled
//! caller calls a hod-assembled variadic function that builds the register
//! save areas and the `va_list` from `VaInfo` as §9.3 specifies and hands
//! it to a C function that takes the arguments with `va_arg`. The C is
//! compiled by `clang -O0 -S -emit-llvm` and then by `llc`.

use std::fmt::Write as _;

use crate::emit::{self, Func, Item};
use crate::ir;
use crate::run::{Harness, Report, TARGET, Tools, compare, compile, item, llvm_tool, must, tail};
use crate::sigs::{Attr, Blobs, Case, Plan, pa, table};
use crate::ty::{I32, I64, I128, Rng, Ty, st, vec};

/// A C type, the IR type clang passes it as on this target, and the
/// attribute clang gives the argument.
struct CType {
    c: &'static str,
    ty: Ty,
    attr: Attr,
}

const DECLARATIONS: &str = "
#include <stdarg.h>
#include <string.h>
struct s1l { long a; };
struct d1 { double a; };
struct big { long a, b, c; };
struct ld1 { long double a; };
typedef int v4i __attribute__((vector_size(16)));
typedef double v2d __attribute__((vector_size(16)));
extern unsigned char hodabi_outa[], hodabi_outb[];
";

/// The C types of the tests. The IR column is what `clang` 19 and 22 emit
/// for an argument of the C type, named or variable, wherever it is: the
/// types whose IR depends on the registers left (a structure of two
/// eightbytes) are not among them.
fn c_types() -> Vec<CType> {
    let plain = |c, ty| CType { c, ty, attr: Attr::None };
    let memory = |c, fields: &[Ty], align| CType {
        c,
        ty: Ty::Ptr,
        attr: Attr::ByVal { pointee: st(fields), align: Some(align), stack_align: None },
    };
    vec![
        plain("long", I64),
        plain("double", Ty::Double),
        plain("int", I32),
        plain("void *", Ty::Ptr),
        plain("long double", Ty::X86Fp80),
        plain("__int128", I128),
        plain("__float128", Ty::Fp128),
        plain("struct s1l", I64),
        plain("struct d1", Ty::Double),
        plain("v4i", vec(I32, 4)),
        plain("v2d", vec(Ty::Double, 2)),
        memory("struct big", &[I64, I64, I64], 8),
        memory("struct ld1", &[Ty::X86Fp80], 16),
    ]
}

const LONG: usize = 0;
const DOUBLE: usize = 1;

/// A variadic signature in C types: indexes into [`c_types`].
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Shape {
    named: Vec<usize>,
    variable: Vec<usize>,
}

fn shapes(types: &[CType], random: usize) -> Vec<Shape> {
    let rep = |ty: usize, count: usize| std::iter::repeat_n(ty, count);
    let mut prefixes: Vec<Vec<usize>> = vec![vec![LONG], vec![DOUBLE]];
    for count in [3, 6, 7, 8, 9] {
        prefixes.push(rep(LONG, count).collect());
        prefixes.push(rep(DOUBLE, count).collect());
    }
    prefixes.push(rep(LONG, 7).chain(rep(DOUBLE, 7)).collect());
    prefixes.push(rep(LONG, 8).chain(rep(DOUBLE, 8)).collect());
    prefixes.push(rep(LONG, 8).chain(rep(DOUBLE, 8)).chain([2]).collect());
    let mut all = Vec::new();
    for named in &prefixes {
        all.push(Shape { named: named.clone(), variable: Vec::new() });
        for ty in 0..types.len() {
            all.push(Shape { named: named.clone(), variable: vec![ty] });
            all.push(Shape { named: named.clone(), variable: vec![ty, LONG, ty, DOUBLE, 2, ty] });
        }
    }
    // Named arguments of every type too.
    for ty in 0..types.len() {
        for fill in [1, 5, 7] {
            let named: Vec<usize> = rep(LONG, fill).chain(rep(DOUBLE, fill)).chain([ty]).collect();
            all.push(Shape { named, variable: vec![2, ty, DOUBLE, ty, LONG] });
        }
    }
    let mut rng = Rng(0x7a);
    for _ in 0..random {
        let mut draw = |most: u64| -> Vec<usize> {
            let count = rng.below(most + 1);
            (0..count).map(|_| rng.below(types.len() as u64) as usize).collect()
        };
        let mut named = draw(9);
        if named.is_empty() {
            named.push(LONG);
        }
        all.push(Shape { named, variable: draw(12) });
    }
    let mut seen = std::collections::HashSet::new();
    all.retain(|shape| seen.insert(shape.clone()));
    all
}

/// The C side: `lf_<n>`, a variadic function that records its arguments,
/// and `vw_<n>`, which records the variable arguments of a `va_list`.
fn walkers(types: &[CType], shapes: &[Shape], plans: &[Plan]) -> String {
    let mut text = String::from(DECLARATIONS);
    for (index, (shape, plan)) in shapes.iter().zip(plans).enumerate() {
        let named = shape.named.len();
        let _ = write!(text, "void lf_{index}(");
        for (n, &ty) in shape.named.iter().enumerate() {
            let _ = write!(text, "{} a{n}, ", types[ty].c);
        }
        let _ = writeln!(text, "...) {{\n  va_list ap;\n  va_start(ap, a{});", named - 1);
        for n in 0..named {
            let _ =
                writeln!(text, "  memcpy(hodabi_outa + {}, &a{n}, sizeof a{n});", plan.args[n].out);
        }
        let walk = |text: &mut String, blob: &str| {
            for (n, &ty) in shape.variable.iter().enumerate() {
                let _ = writeln!(
                    text,
                    "  {{ {} v = va_arg(ap, {}); memcpy({blob} + {}, &v, sizeof v); }}",
                    types[ty].c,
                    types[ty].c,
                    plan.args[named + n].out
                );
            }
        };
        walk(&mut text, "hodabi_outa");
        text.push_str("  va_end(ap);\n}\n");
        let _ = writeln!(text, "void vw_{index}(va_list ap) {{");
        walk(&mut text, "hodabi_outb");
        text.push_str("}\n");
    }
    text
}

/// `spec/abi-x64.md` §9.1–§9.4: variable arguments as a C callee takes
/// them, and the `va_list` a hod callee builds from `VaInfo`, against
/// clang and llc 22 and 19.
#[test]
fn variadic_against_c() {
    let Some(tools) = Tools::find() else { return };
    let types = c_types();
    let shapes = shapes(&types, if crate::grid::full() { 1500 } else { 150 });
    let cases: Vec<Case> = shapes
        .iter()
        .map(|shape| {
            let params = shape.named.iter().chain(&shape.variable);
            Case::new(params.map(|&ty| pa(types[ty].ty.clone(), types[ty].attr.clone())).collect())
                .variadic(shape.named.len())
        })
        .collect();

    let mut table = table();
    let mut blobs = Blobs::default();
    let mut rng = Rng(0x7a11);
    let plans: Vec<Plan> =
        cases.iter().map(|case| Plan::new(case, &mut table, &mut blobs, &mut rng)).collect();
    let mut asm = String::new();
    emit::prelude(&mut asm);
    for (index, plan) in plans.iter().enumerate() {
        let args: Vec<Item> = plan.args.iter().map(|slot| item(slot, false, false)).collect();
        let names = [
            format!("hc_{index}"),
            format!("lf_{index}"),
            format!("hf_{index}"),
            format!("vw_{index}"),
        ];
        let func = Func {
            name: &names[0],
            peer: &names[1],
            out: ir::OUT_A,
            asg: &plan.asg,
            args: &args,
            ret: None,
            variadic: true,
            avx: false,
        };
        emit::caller(&mut asm, func);
        let callee =
            Func { name: &names[2], peer: &names[3], out: ir::OUT_B, variadic: false, ..func };
        let named = plan.case.fixed.expect("a variadic case");
        emit::variadic_callee(&mut asm, callee, named, plan.asg.va);
    }
    emit::epilogue(&mut asm);
    let harness = Harness::new("abi-variadic", blobs, plans.len(), &asm, &tools);
    let scratch = &harness.scratch;
    let c_source = scratch.write("walkers.c", walkers(&types, &shapes, &plans));
    let callers = scratch.write("callers.ll", ir::callers(&plans));

    let mut report = Report::default();
    for (config, llc) in &tools.llc {
        let Some(clang) = llvm_tool(config.major, "clang") else { return };
        let tag = config.tag();
        let walkers_ir = scratch.join(format!("walkers{}.ll", config.major));
        let emitted = hod_testutil::run(
            &clang,
            [
                TARGET.as_ref(),
                "-O0".as_ref(),
                "-S".as_ref(),
                "-emit-llvm".as_ref(),
                c_source.as_os_str(),
                "-o".as_ref(),
                walkers_ir.as_os_str(),
            ],
        );
        must("compiling the C walkers", &emitted);
        let objects =
            [scratch.join(format!("walkers{tag}.o")), scratch.join(format!("callers{tag}.o"))];
        must("llc on the walkers", &compile(llc, *config, &walkers_ir, &objects[0]));
        let compiled = compile(llc, *config, &callers, &objects[1]);
        assert!(compiled.success(), "llc {tag} on the callers:\n{}", tail(&compiled.stderr_text()));
        // §4.2: LLVM 19 keeps a wide integer whole where LLVM 22 splits
        // it; those steps do not run against LLVM 19.
        let skip = |index: usize| config.major < 22 && plans[index].case.has_wide_general();
        let wanted: Vec<bool> = (0..plans.len() * 2).map(|step| !skip(step / 2)).collect();
        let (out, crashed) =
            harness.execute(&tag, &objects, &wanted, &tools).unwrap_or_else(|(problem, _)| {
                panic!("[llc {tag}] {problem}");
            });
        let size = harness.blobs.out_size as usize;
        for (index, plan) in plans.iter().enumerate() {
            if skip(index) {
                continue;
            }
            for hod_calls in [true, false] {
                let blob = if hod_calls { &out[..size] } else { &out[size..] };
                let problem = if crashed[index * 2 + usize::from(!hod_calls)] {
                    Some("the step crashed".to_string())
                } else {
                    compare(plan, hod_calls, blob)
                };
                report.count(&tag, hod_calls);
                if let Some(problem) = problem {
                    let side = if hod_calls { "hod calls C" } else { "LLVM calls hod, C walks" };
                    report.failures.push(format!(
                        "[llc {tag}] case {index}, {side}: `{}`: {problem}\n    va: {:?}",
                        plan.case.text(),
                        plan.asg.va
                    ));
                }
            }
        }
    }
    eprintln!(
        "variadic: {} signatures, each with hod calling C and with LLVM calling hod; {}",
        plans.len(),
        report.summary()
    );
    report.check("variadic");
}
