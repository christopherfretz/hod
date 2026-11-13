//! What x86-64 modules bring to the interpreter (WP-138): `x86_fp80`
//! values in registers, memory and initializers (§5.11), `byval`
//! arguments (§5.8), and function addresses aligned as their `align`
//! says (§4). The programs are LLVM text read by `hod_ll`, as clang and
//! kiln write them for `x86_64-unknown-linux-gnu`.

use hod_interp::{Interp, Program, Stop, Trap, Val};
use hod_ll::{BodyParser, ParseOptions, parse_module};

const HEADER: &str = "target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128\"\n\
                      target triple = \"x86_64-unknown-linux-gnu\"\n";

/// Runs `@main` of `HEADER` + `text`.
fn run(text: &str) -> Result<Option<Val>, Stop> {
    let source = format!("{HEADER}{text}");
    let mut parsed = parse_module(source.as_bytes(), &ParseOptions::default()).expect("module");
    let mut parser = BodyParser::new(&parsed.text);
    let mut bodies = Vec::new();
    for function in parsed.text.functions() {
        if let Some(body) = parser.parse(&mut parsed.module, function).expect("body") {
            bodies.push(body);
        }
    }
    let module = &parsed.module;
    let program = Program::with_bodies(module, &bodies).expect("program");
    program.verify().expect("verifies");
    let main = module.find_symbol(b"main").expect("main");
    Interp::new(&program).run(main, &[]).result
}

fn int(text: &str) -> u128 {
    match run(text) {
        Ok(Some(Val::Int { value, .. })) => value,
        other => panic!("{other:?}"),
    }
}

/// §5.11: arithmetic, compares and conversions with 80-bit semantics:
/// `1 + 2^-63` exists in `x86_fp80` and not in `double`.
#[test]
fn extended_precision() {
    let bits = |text: &str| int(&format!("define i80 @main() {{\n{text}\n}}\n"));
    // 1 + 2^-63 - 1 = 2^-63.
    let small = bits(
        "  %a = fadd x86_fp80 0xK3FFF8000000000000000, 0xK3FC08000000000000000\n\
           %b = fsub x86_fp80 %a, 0xK3FFF8000000000000000\n\
           %r = bitcast x86_fp80 %b to i80\n  ret i80 %r",
    );
    assert_eq!(small, 0x3FC0_8000_0000_0000_0000);
    // 1 / 3 rounds at the 64th bit.
    let third = bits(
        "  %q = fdiv x86_fp80 0xK3FFF8000000000000000, 0xK4000C000000000000000\n\
           %r = bitcast x86_fp80 %q to i80\n  ret i80 %r",
    );
    assert_eq!(third, 0x3FFD_AAAA_AAAA_AAAA_AAAB);
    // fneg flips the sign; frem is fmod; an i64 is exact.
    let rem = bits(
        "  %x = sitofp i64 -9007199254740993 to x86_fp80\n\
           %n = fneg x86_fp80 %x\n\
           %m = frem x86_fp80 %n, 0xK4002A000000000000000\n\
           %r = bitcast x86_fp80 %m to i80\n  ret i80 %r",
    );
    // 9007199254740993 mod 10 = 3.
    assert_eq!(rem, 0x4000_C000_0000_0000_0000);
    let conversions = int("define i64 @main() {\n\
           %x = fpext double 2.5 to x86_fp80\n\
           %y = fmul x86_fp80 %x, 0xK4000C000000000000000\n\
           %c = fcmp olt x86_fp80 %y, 0xK4001F800000000000000\n\
           %i = fptosi x86_fp80 %y to i64\n\
           %d = fptrunc x86_fp80 %y to double\n\
           %e = fptosi double %d to i64\n\
           %s = add i64 %i, %e\n\
           %z = zext i1 %c to i64\n\
           %t = add i64 %s, %z\n\
           ret i64 %t\n}\n");
    // 7.5 truncates to 7, twice, and 7.5 < 7.75.
    assert_eq!(conversions, 15);
    // fptoui of a negative value traps (LangRef: poison).
    let negative = run("define i32 @main() {\n\
           %i = fptoui x86_fp80 0xKBFFF8000000000000000 to i32\n  ret i32 %i\n}\n");
    assert_eq!(negative, Err(Stop::Trap(Trap::FloatToIntOverflow)));
    // A computed NaN traps (§5.1).
    let nan = run("define i32 @main() {\n\
           %n = fsub x86_fp80 0xK7FFF8000000000000000, 0xK7FFF8000000000000000\n\
           %c = fcmp uno x86_fp80 %n, %n\n  %i = zext i1 %c to i32\n  ret i32 %i\n}\n");
    assert_eq!(nan, Err(Stop::Trap(Trap::NanResult)));
}

/// §4, §5.11: ten bytes in memory at a 16-byte stride, in globals'
/// initializers and in aggregates.
#[test]
fn in_memory() {
    let value = int("%S = type { i64, x86_fp80 }\n\
         @s = global %S { i64 3, x86_fp80 0xK4000C000000000000000 }, align 16\n\
         @a = global [2 x x86_fp80] [x86_fp80 0xK3FFF8000000000000000, x86_fp80 0xK4000C000000000000000], align 16\n\
         define i64 @main() {\n\
           %p = getelementptr inbounds %S, ptr @s, i32 0, i32 1\n\
           %x = load x86_fp80, ptr %p, align 16\n\
           %q = getelementptr inbounds [2 x x86_fp80], ptr @a, i64 0, i64 1\n\
           %y = load x86_fp80, ptr %q, align 16\n\
           %z = fadd x86_fp80 %x, %y\n\
           %slot = alloca x86_fp80, align 16\n\
           store x86_fp80 %z, ptr %slot, align 16\n\
           %w = load x86_fp80, ptr %slot, align 16\n\
           %agg = load %S, ptr @s, align 16\n\
           %f = extractvalue %S %agg, 1\n\
           %g = fadd x86_fp80 %w, %f\n\
           %i = fptosi x86_fp80 %g to i64\n\
           ret i64 %i\n}\n");
    assert_eq!(value, 9);
    // The stride is 16: the second element is at offset 16.
    let stride = int(
        "@a = global [2 x x86_fp80] [x86_fp80 0xK3FFF8000000000000000, x86_fp80 0xK4000C000000000000000], align 16\n\
         define i64 @main() {\n\
           %q = getelementptr inbounds i8, ptr @a, i64 16\n\
           %y = load x86_fp80, ptr %q, align 16\n\
           %i = fptosi x86_fp80 %y to i64\n  ret i64 %i\n}\n",
    );
    assert_eq!(stride, 3);
}

/// §5.8: a `byval(T)` argument is a copy: the callee's writes do not
/// reach the caller, and the copy holds what the caller wrote.
#[test]
fn byval_is_a_copy() {
    let value = int("%P = type { i64, i64, i64 }\n\
         define i64 @callee(ptr byval(%P) align 8 %p) {\n\
           %a = load i64, ptr %p, align 8\n\
           store i64 100, ptr %p, align 8\n\
           %q = getelementptr inbounds i8, ptr %p, i64 16\n\
           %c = load i64, ptr %q, align 8\n\
           %s = add i64 %a, %c\n  ret i64 %s\n}\n\
         define i64 @main() {\n\
           %v = alloca %P, align 8\n\
           store i64 1, ptr %v, align 8\n\
           %q = getelementptr inbounds i8, ptr %v, i64 16\n\
           store i64 20, ptr %q, align 8\n\
           %r = call i64 @callee(ptr byval(%P) align 8 %v)\n\
           %again = load i64, ptr %v, align 8\n\
           %s = add i64 %r, %again\n  ret i64 %s\n}\n");
    // 1 + 20, plus the caller's unchanged 1.
    assert_eq!(value, 22);
    // An unwritten field of the copy is still unwritten.
    let unwritten = run("%P = type { i64, i64 }\n\
         define i64 @callee(ptr byval(%P) %p) {\n\
           %q = getelementptr inbounds i8, ptr %p, i64 8\n\
           %c = load i64, ptr %q, align 8\n  ret i64 %c\n}\n\
         define i64 @main() {\n\
           %v = alloca %P, align 8\n\
           store i64 1, ptr %v, align 8\n\
           %r = call i64 @callee(ptr byval(%P) %v)\n  ret i64 %r\n}\n");
    assert_eq!(unwritten, Err(Stop::Trap(Trap::UninitializedLoad)));
}

/// §4: x86-64's Itanium ABI marks a virtual member function pointer by
/// the low bit; a function with `align 2` never has it.
#[test]
fn function_addresses_are_aligned() {
    let low = int("define void @f() align 2 {\n  ret void\n}\n\
         define void @g() align 16 {\n  ret void\n}\n\
         define i64 @main() {\n\
           %a = and i64 ptrtoint (ptr @f to i64), 1\n\
           %b = and i64 ptrtoint (ptr @g to i64), 15\n\
           %s = or i64 %a, %b\n  ret i64 %s\n}\n");
    assert_eq!(low, 0);
}
