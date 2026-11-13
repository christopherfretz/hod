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

/// Runs `@main` of `header` + `text` with the standard host functions;
/// the result and the values passed to `sink`.
fn run_sinking(header: &str, text: &str) -> (Result<Option<Val>, Stop>, Vec<u64>) {
    let source = format!("{header}{text}");
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
    let mut interp = Interp::new(&program);
    hod_interp::host::register(&mut interp);
    let outcome = interp.run(main, &[]);
    let sunk = outcome
        .trace
        .iter()
        .filter_map(|event| event.args.first().and_then(Val::as_u64))
        .collect();
    (outcome.result, sunk)
}

/// A variadic callee that sinks the four fields of its `va_list` (the
/// pointers as offsets from the save area and from each other) and the
/// first variable argument in a register, then `@main` calling it.
const LAYOUT: &str = "%S = type { i64, i64, i64 }\n\
    declare void @sink(i64)\n\
    declare void @llvm.va_start.p0(ptr)\n\
    declare void @llvm.va_end.p0(ptr)\n\
    define void @f(i32 %a, double %b, x86_fp80 %c, ptr byval(%S) align 8 %d, ...) {\n\
      %ap = alloca [24 x i8], align 16\n\
      call void @llvm.va_start.p0(ptr %ap)\n\
      %gp = load i32, ptr %ap\n\
      %gp64 = zext i32 %gp to i64\n\
      call void @sink(i64 %gp64)\n\
      %fpp = getelementptr i8, ptr %ap, i64 4\n\
      %fp = load i32, ptr %fpp\n\
      %fp64 = zext i32 %fp to i64\n\
      call void @sink(i64 %fp64)\n\
      %ovp = getelementptr i8, ptr %ap, i64 8\n\
      %ov = load ptr, ptr %ovp\n\
      %rsp = getelementptr i8, ptr %ap, i64 16\n\
      %rs = load ptr, ptr %rsp\n\
      %gpslot = getelementptr i8, ptr %rs, i64 %gp64\n\
      %first = load i64, ptr %gpslot\n\
      call void @sink(i64 %first)\n\
      %fpslot = getelementptr i8, ptr %rs, i64 %fp64\n\
      %second = load double, ptr %fpslot\n\
      %second.i = bitcast double %second to i64\n\
      call void @sink(i64 %second.i)\n\
      %ovx = getelementptr i8, ptr %ov, i64 8\n\
      %third = load x86_fp80, ptr %ovx, align 16\n\
      %third.i = fptosi x86_fp80 %third to i64\n\
      call void @sink(i64 %third.i)\n\
      %next = getelementptr i8, ptr %rs, i64 16\n\
      %fourth = load i32, ptr %next\n\
      %fourth.i = sext i32 %fourth to i64\n\
      call void @sink(i64 %fourth.i)\n\
      %ovi = ptrtoint ptr %ov to i64\n\
      %low = and i64 %ovi, 15\n\
      call void @sink(i64 %low)\n\
      call void @llvm.va_end.p0(ptr %ap)\n\
      ret void\n\
    }\n";

/// §5.12: after the named `(i32, double, x86_fp80, byval %S)`, `va_start`
/// gives `gp_offset` 8, `fp_offset` 64, and an overflow area after the 16
/// bytes of the `x86_fp80` and the 24 of the copy (40, rounded to 8);
/// the variable arguments are where the psABI puts them.
#[test]
fn variadic_layout() {
    let (result, sunk) = run_sinking(
        HEADER,
        &format!(
            "{LAYOUT}define void @main() {{\n\
               %s = alloca %S, align 8\n\
               store %S zeroinitializer, ptr %s\n\
               call void (i32, double, x86_fp80, ptr, ...) @f(i32 1, double 2.0, x86_fp80 0xK3FFF8000000000000000, ptr byval(%S) align 8 %s, i64 77, double 1.5, x86_fp80 0xK4001A000000000000000, i32 -9)\n\
               ret void\n}}\n"
        ),
    );
    assert_eq!(result, Ok(None));
    // The overflow area starts at 40 of the 16-aligned stack-argument
    // area; the `x86_fp80` is in the next 16-aligned slot, at 48; the
    // `i32` is in the second general register, `rsi`, at 16.
    assert_eq!(sunk, [8, 64, 77, 1.5f64.to_bits(), 5, (-9i64) as u64, 8]);
}

/// §5.12: only what the caller owes is written; `va_end` and the end of
/// the frame make the list unusable; `va_copy` copies it.
#[test]
fn va_end_and_va_copy() {
    let callee = |body: &str| {
        format!(
            "declare void @sink(i64)\n\
             declare void @llvm.va_start.p0(ptr)\n\
             declare void @llvm.va_end.p0(ptr)\n\
             declare void @llvm.va_copy.p0(ptr, ptr)\n\
             @keep = global ptr null\n\
             define i64 @f(i32 %n, ...) {{\n\
               %ap = alloca [24 x i8], align 16\n\
               %aq = alloca [24 x i8], align 16\n\
               call void @llvm.va_start.p0(ptr %ap)\n\
               %rsp = getelementptr i8, ptr %ap, i64 16\n\
               %rs = load ptr, ptr %rsp\n\
               {body}\n\
             }}\n"
        )
    };
    let main = |args: &str| {
        format!("define i64 @main() {{\n  %r = call i64 (i32, ...) @f(i32 0, {args})\n  ret i64 %r\n}}\n")
    };
    let run = |body: &str, args: &str| run_sinking(HEADER, &(callee(body) + &main(args))).0;
    // An `i32` passed, an `i64` read: the upper half was never written.
    let wide = "%slot = getelementptr i8, ptr %rs, i64 8\n  %v = load i64, ptr %slot\n  ret i64 %v";
    assert_eq!(run(wide, "i32 5"), Err(Stop::Trap(Trap::UninitializedLoad)));
    assert_eq!(run(wide, "i64 5"), Ok(Some(Val::i64(5))));
    // `signext` owes 32 bits.
    let ext = "%slot = getelementptr i8, ptr %rs, i64 8\n  %v = load i32, ptr %slot\n  %w = zext i32 %v to i64\n  ret i64 %w";
    assert_eq!(run(ext, "i8 signext -1"), Ok(Some(Val::i64(0xffff_ffff))));
    // A `float` passed, a `double` read.
    let double = "%slot = getelementptr i8, ptr %rs, i64 48\n  %v = load double, ptr %slot\n  %w = bitcast double %v to i64\n  ret i64 %w";
    assert_eq!(run(double, "float 1.0"), Err(Stop::Trap(Trap::UninitializedLoad)));
    // Past the variable arguments: nothing was written.
    let past = "%slot = getelementptr i8, ptr %rs, i64 16\n  %v = load i64, ptr %slot\n  ret i64 %v";
    assert_eq!(run(past, "i64 5"), Err(Stop::Trap(Trap::UninitializedLoad)));
    // `va_copy` copies; `va_end` makes the list unwritten.
    let copy = "call void @llvm.va_copy.p0(ptr %aq, ptr %ap)\n  call void @llvm.va_end.p0(ptr %ap)\n  %v = load i32, ptr %aq\n  %w = zext i32 %v to i64\n  ret i64 %w";
    assert_eq!(run(copy, "i64 5"), Ok(Some(Val::i64(8))));
    let ended = "call void @llvm.va_end.p0(ptr %ap)\n  %v = load i32, ptr %ap\n  %w = zext i32 %v to i64\n  ret i64 %w";
    assert_eq!(run(ended, "i64 5"), Err(Stop::Trap(Trap::UninitializedLoad)));
    // The save area dies with the frame.
    let escape = "store ptr %rs, ptr @keep\n  ret i64 0";
    let after = callee(escape)
        + "define i64 @main() {\n  %r = call i64 (i32, ...) @f(i32 0, i64 5)\n  \
           %p = load ptr, ptr @keep\n  %q = getelementptr i8, ptr %p, i64 8\n  \
           %v = load i64, ptr %q\n  ret i64 %v\n}\n";
    assert_eq!(run_sinking(HEADER, &after).0, Err(Stop::Trap(Trap::UseAfterFree)));
}

/// §5.8, §5.12, §8: a variadic host function gets every argument; what
/// the model does not cover is refused by name; AArch64 has no model.
#[test]
fn variadic_refusals() {
    // A variadic declaration registered as a host: `sink` declared `...`.
    let (result, sunk) = run_sinking(
        HEADER,
        "declare void @sink(i64, ...)\n\
         define void @main() {\n  call void (i64, ...) @sink(i64 3, i64 4)\n  ret void\n}\n",
    );
    // The standard `sink` takes one `i64`: two arguments are its refusal.
    assert_eq!((result, sunk), (Err(Stop::Trap(Trap::Host)), vec![]));
    let def = "declare void @llvm.va_start.p0(ptr)\n\
               define void @f(i32 %n, ...) {\n  ret void\n}\n";
    let call = |args: &str| {
        format!("{def}define void @main() {{\n  call void (i32, ...) @f(i32 0, {args})\n  ret void\n}}\n")
    };
    let reason = |header: &str, text: &str| match run_sinking(header, text).0 {
        Err(Stop::Unsupported(reason)) => reason,
        other => panic!("{other:?}"),
    };
    assert_eq!(reason(HEADER, &call("{ i64, i64 } zeroinitializer")), "call:variadic-type");
    assert_eq!(reason(HEADER, &call("i1 true")), "call:variadic-type");
    assert_eq!(reason(HEADER, &call("i96 1")), "call:variadic-type");
    assert_eq!(reason(HEADER, &call("<3 x i32> zeroinitializer")), "call:variadic-type");
    assert_eq!(reason(HEADER, &call("ptr nest null")), "call:variadic-attr");
    let aarch64 = "target datalayout = \"e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32\"\n\
                   target triple = \"aarch64-unknown-linux-gnu\"\n";
    assert_eq!(reason(aarch64, &call("i64 1")), "call:variadic");
}

/// §5.11: the `x86_fp80` intrinsics, by hand.
#[test]
fn x87_intrinsics() {
    let f80 = |expr: &str| {
        let text = format!(
            "declare x86_fp80 @llvm.sqrt.f80(x86_fp80)\n\
             declare x86_fp80 @llvm.floor.f80(x86_fp80)\n\
             declare x86_fp80 @llvm.round.f80(x86_fp80)\n\
             declare x86_fp80 @llvm.roundeven.f80(x86_fp80)\n\
             declare x86_fp80 @llvm.fma.f80(x86_fp80, x86_fp80, x86_fp80)\n\
             declare x86_fp80 @llvm.maxnum.f80(x86_fp80, x86_fp80)\n\
             declare i1 @llvm.is.fpclass.f80(x86_fp80, i32)\n\
             define i80 @main() {{\n  %x = {expr}\n  %r = bitcast x86_fp80 %x to i80\n  ret i80 %r\n}}\n"
        );
        match run(&text) {
            Ok(Some(Val::Int { value, .. })) => Ok(value),
            Ok(other) => panic!("{other:?}"),
            Err(stop) => Err(stop),
        }
    };
    // sqrt(2) rounded to 64 bits; floor(-2.5) = -3; round(2.5) = 3;
    // roundeven(2.5) = 2.
    assert_eq!(f80("call x86_fp80 @llvm.sqrt.f80(x86_fp80 0xK40008000000000000000)"), Ok(0x3FFF_B504_F333_F9DE_6484));
    assert_eq!(f80("call x86_fp80 @llvm.floor.f80(x86_fp80 0xKC000A000000000000000)"), Ok(0xC000_C000_0000_0000_0000));
    assert_eq!(f80("call x86_fp80 @llvm.round.f80(x86_fp80 0xK4000A000000000000000)"), Ok(0x4000_C000_0000_0000_0000));
    assert_eq!(f80("call x86_fp80 @llvm.roundeven.f80(x86_fp80 0xK4000A000000000000000)"), Ok(0x4000_8000_0000_0000_0000));
    // (1 + 2^-63)(1 - 2^-63) - 1 = -2^-126 exactly: fused, not zero.
    assert_eq!(
        f80("call x86_fp80 @llvm.fma.f80(x86_fp80 0xK3FFF8000000000000001, x86_fp80 0xK3FFEFFFFFFFFFFFFFFFE, x86_fp80 0xKBFFF8000000000000000)"),
        Ok(0xBF81_8000_0000_0000_0000)
    );
    assert_eq!(f80("call x86_fp80 @llvm.sqrt.f80(x86_fp80 0xKBFFF8000000000000000)"), Err(Stop::Trap(Trap::NanResult)));
    assert_eq!(
        f80("call x86_fp80 @llvm.maxnum.f80(x86_fp80 0xK00000000000000000000, x86_fp80 0xK80000000000000000000)"),
        Err(Stop::Trap(Trap::ZeroSign))
    );
    assert_eq!(
        f80("call x86_fp80 @llvm.maxnum.f80(x86_fp80 0xK7FFFC000000000000000, x86_fp80 0xK3FFF8000000000000000)"),
        Ok(0x3FFF_8000_0000_0000_0000)
    );
    // An unnormal has no class LLVM agrees on.
    let class = run("declare i1 @llvm.is.fpclass.f80(x86_fp80, i32)\n\
         define i1 @main() {\n  %c = call i1 @llvm.is.fpclass.f80(x86_fp80 0xK3FFF0000000000000001, i32 3)\n  ret i1 %c\n}\n");
    assert_eq!(class, Err(Stop::Unsupported("intrinsic-x86_fp80".into())));
}
