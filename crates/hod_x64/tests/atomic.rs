//! x86-64 atomics (WP-133, `spec/codegen.md` §5.6.10 "Atomics"): every
//! atomic operation at every width and ordering, single-threaded,
//! against `hod_interp` and `llc -O0 -fast-isel=0` 22 and 19 under the
//! emulated runner (`common/aggr_gen.rs`); 16 bytes with `+cx16` on a
//! runner whose CPU model has it (`common/atomic_wide.rs`); contended
//! programs on real threads (`common/atomic_threads.rs`); the exact
//! instruction forms (`lock`, `xchg` for a `seq_cst` store, `mfence`);
//! what is refused, by name.

#![allow(clippy::many_single_char_names, reason = "`.ll` text built by hand")]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;

#[path = "common/atomic_threads.rs"]
mod atomic_threads;

#[path = "common/atomic_wide.rs"]
mod atomic_wide;

use std::fmt::Write as _;
use std::process::Command;
use std::time::Duration;

use aggr_gen::{
    PIE_FLAGS, Report, Rng, Suite, T, assert_agrees, bytes_of, edges, header, parse, pic, run,
    value,
};
use hod_cg::{CgError, Stress};
use hod_testutil::{Scratch, run_with};
use hod_x64::atomic;

/// The orderings an `atomicrmw` or a `cmpxchg` success takes.
pub const RMW_ORDERS: [&str; 5] = ["monotonic", "acquire", "release", "acq_rel", "seq_cst"];
/// The orderings a `cmpxchg` failure takes.
pub const FAIL_ORDERS: [&str; 3] = ["monotonic", "acquire", "seq_cst"];
/// The integer operations.
pub const INT_OPS: [&str; 15] = [
    "xchg",
    "add",
    "sub",
    "and",
    "nand",
    "or",
    "xor",
    "max",
    "min",
    "umax",
    "umin",
    "uinc_wrap",
    "udec_wrap",
    "usub_cond",
    "usub_sat",
];
/// The integer operations LLVM 19 reads (`usub_cond` and `usub_sat`
/// are LLVM 20's: `common/atomic_wide.rs` checks them at every width).
const LLVM19_OPS: [&str; 13] = [
    "xchg",
    "add",
    "sub",
    "and",
    "nand",
    "or",
    "xor",
    "max",
    "min",
    "umax",
    "umin",
    "uinc_wrap",
    "udec_wrap",
];
/// The integer widths of one register.
const WIDTHS: [u32; 4] = [8, 16, 32, 64];

/// A type an atomic operation works on, carried in the suite as the
/// integer of its bits.
#[derive(Clone, Copy)]
enum Ty {
    Int(u32),
    Ptr,
    Half,
    BFloat,
    Float,
    Double,
}

impl Ty {
    fn bits(self) -> u32 {
        match self {
            Ty::Int(b) => b,
            Ty::Half | Ty::BFloat => 16,
            Ty::Float => 32,
            Ty::Ptr | Ty::Double => 64,
        }
    }

    fn ll(self) -> String {
        match self {
            Ty::Int(b) => format!("i{b}"),
            Ty::Ptr => "ptr".into(),
            Ty::Half => "half".into(),
            Ty::BFloat => "bfloat".into(),
            Ty::Float => "float".into(),
            Ty::Double => "double".into(),
        }
    }

    fn align(self) -> u32 {
        self.bits() / 8
    }

    /// `%<name>` of this type from the integer `%<from>`: a floating
    /// value through a stack slot (`bitcast` between integers and floats
    /// is WP-127's).
    fn of_int(self, name: &str, from: &str) -> String {
        let b = self.bits();
        match self {
            Ty::Int(_) => format!("  %{name} = add i{b} %{from}, 0\n"),
            Ty::Ptr => format!("  %{name} = inttoptr i64 %{from} to ptr\n"),
            _ => format!(
                "  %{name}.s = alloca i64, align 8\n  store i{b} %{from}, ptr %{name}.s\n  \
                 %{name} = load {}, ptr %{name}.s\n",
                self.ll()
            ),
        }
    }

    /// The integer `%<name>` from `%<from>` of this type.
    fn to_int(self, name: &str, from: &str) -> String {
        let b = self.bits();
        match self {
            Ty::Int(_) => format!("  %{name} = add i{b} %{from}, 0\n"),
            Ty::Ptr => format!("  %{name} = ptrtoint ptr %{from} to i64\n"),
            _ => format!(
                "  %{name}.s = alloca i64, align 8\n  store {} %{from}, ptr %{name}.s\n  \
                 %{name} = load i{b}, ptr %{name}.s\n",
                self.ll()
            ),
        }
    }
}

/// Bit patterns worth trying for a floating-point format (zeros, ones,
/// fractions, infinities, a NaN, the largest finite, a subnormal).
fn float_edges(ty: Ty) -> Vec<u128> {
    match ty {
        Ty::Half => {
            vec![0, 0x8000, 0x3c00, 0xbc00, 0x4100, 0xc500, 0x7c00, 0xfc00, 0x7e00, 0x7bff, 1]
        }
        Ty::BFloat => vec![0, 0x8000, 0x3f80, 0xbf80, 0x4020, 0x7f80, 0xff80, 0x7fc0, 0x7f7f],
        Ty::Float => vec![
            0,
            0x8000_0000,
            0x3f80_0000,
            0xbf80_0000,
            0x4020_0000,
            0xc0a0_0000,
            0x7f80_0000,
            0xff80_0000,
            0x7fc0_0000,
            0x7f7f_ffff,
            1,
        ],
        _ => vec![
            0,
            0x8000_0000_0000_0000,
            0x3ff0_0000_0000_0000,
            0xbff0_0000_0000_0000,
            0x4004_0000_0000_0000,
            0x7ff0_0000_0000_0000,
            0xfff0_0000_0000_0000,
            0x7ff8_0000_0000_0000,
            0x7fef_ffff_ffff_ffff,
            1,
        ],
    }
}

/// A value of `ty`: an edge (half the time) or random bits.
fn pick(rng: &mut Rng, ty: Ty) -> Vec<u8> {
    let bits = ty.bits();
    match ty {
        Ty::Int(_) | Ty::Ptr => bytes_of(&value(rng, bits), bits),
        _ => {
            let w = if rng.chance(70) {
                *rng.pick(&float_edges(ty))
            } else {
                u128::from(rng.next()) & ((1u128 << bits) - 1)
            };
            w.to_le_bytes()[..(bits / 8) as usize].to_vec()
        }
    }
}

/// The `{ old, new }` result of an operation on `ty`.
fn pair(ty: Ty) -> T {
    T::S(vec![T::I(ty.bits()), T::I(ty.bits())], false)
}

/// The prologue of a core: `%pa`, `%pb`, `%pc` of `ty` from `%a`, `%b`,
/// `%c`, and the cell `%p` holding `%pa`.
fn prologue(ty: Ty, operands: usize) -> String {
    let mut core = String::new();
    for name in ["a", "b", "c"].iter().take(operands) {
        core.push_str(&ty.of_int(&format!("p{name}"), name));
    }
    let (t, a) = (ty.ll(), ty.align());
    writeln!(core, "  %p = alloca {t}, align {a}\n  store {t} %pa, ptr %p, align {a}").unwrap();
    core
}

/// Adds every `atomicrmw` of `ops` on `ty` under every ordering, with
/// `cases` random cases each (and `(x, x)`, which the wrapping and
/// conditional operations treat specially).
fn add_rmw(s: &mut Suite, rng: &mut Rng, ty: Ty, ops: &[&str], cases: usize) {
    let (t, a, b) = (ty.ll(), ty.align(), ty.bits());
    for op in ops {
        for (k, order) in RMW_ORDERS.iter().enumerate() {
            let volatile = if k == 2 { "volatile " } else { "" };
            let mut core = prologue(ty, 2);
            writeln!(
                core,
                "  %old = atomicrmw {volatile}{op} ptr %p, {t} %pb {order}, align {a}\n  \
                 %new = load {t}, ptr %p, align {a}"
            )
            .unwrap();
            core.push_str(&ty.to_int("xo", "old"));
            core.push_str(&ty.to_int("xn", "new"));
            writeln!(
                core,
                "  %r0 = insertvalue {{ i{b}, i{b} }} poison, i{b} %xo, 0\n  \
                 %r = insertvalue {{ i{b}, i{b} }} %r0, i{b} %xn, 1"
            )
            .unwrap();
            let f = s.add(&[T::I(b), T::I(b)], pair(ty), &core);
            for _ in 0..cases {
                let (x, y) = (pick(rng, ty), pick(rng, ty));
                s.case(f, vec![x, y]);
            }
            let x = pick(rng, ty);
            s.case(f, vec![x.clone(), x]);
        }
    }
}

/// Adds atomic loads and stores of `ty` under every ordering (and a
/// volatile one of each).
fn add_load_store(s: &mut Suite, rng: &mut Rng, ty: Ty) {
    let (t, a, b) = (ty.ll(), ty.align(), ty.bits());
    for (order, volatile) in [
        ("unordered", ""),
        ("monotonic", ""),
        ("acquire", ""),
        ("seq_cst", ""),
        ("seq_cst", "volatile "),
    ] {
        let mut core = prologue(ty, 1);
        writeln!(core, "  %v = load atomic {volatile}{t}, ptr %p {order}, align {a}").unwrap();
        core.push_str(&ty.to_int("r", "v"));
        let f = s.add(&[T::I(b)], T::I(b), &core);
        for _ in 0..4 {
            s.case(f, vec![pick(rng, ty)]);
        }
    }
    for (order, volatile) in [
        ("unordered", ""),
        ("monotonic", ""),
        ("release", ""),
        ("seq_cst", ""),
        ("monotonic", "volatile "),
        ("seq_cst", "volatile "),
    ] {
        let mut core = prologue(ty, 2);
        writeln!(
            core,
            "  store atomic {volatile}{t} %pb, ptr %p {order}, align {a}\n  \
             %v = load {t}, ptr %p, align {a}"
        )
        .unwrap();
        core.push_str(&ty.to_int("r", "v"));
        let f = s.add(&[T::I(b), T::I(b)], T::I(b), &core);
        for _ in 0..4 {
            s.case(f, vec![pick(rng, ty), pick(rng, ty)]);
        }
    }
}

/// Adds `cmpxchg` (strong and weak) of `ty` under every pair of
/// orderings: cases that succeed and cases that fail.
fn add_cmpxchg(s: &mut Suite, rng: &mut Rng, ty: Ty) {
    let (t, a, b) = (ty.ll(), ty.align(), ty.bits());
    let ret = T::S(vec![T::I(b), T::I(1), T::I(b)], false);
    for weak in ["", "weak "] {
        for success in RMW_ORDERS {
            for failure in FAIL_ORDERS {
                let volatile = if success == "acq_rel" { "volatile " } else { "" };
                let mut core = prologue(ty, 3);
                writeln!(
                    core,
                    "  %x = cmpxchg {weak}{volatile}ptr %p, {t} %pb, {t} %pc {success} {failure}, align {a}\n  \
                     %old = extractvalue {{ {t}, i1 }} %x, 0\n  \
                     %ok = extractvalue {{ {t}, i1 }} %x, 1\n  \
                     %new = load {t}, ptr %p, align {a}"
                )
                .unwrap();
                core.push_str(&ty.to_int("xo", "old"));
                core.push_str(&ty.to_int("xn", "new"));
                writeln!(
                    core,
                    "  %r0 = insertvalue {{ i{b}, i1, i{b} }} poison, i{b} %xo, 0\n  \
                     %r1 = insertvalue {{ i{b}, i1, i{b} }} %r0, i1 %ok, 1\n  \
                     %r = insertvalue {{ i{b}, i1, i{b} }} %r1, i{b} %xn, 2"
                )
                .unwrap();
                let f = s.add(&[T::I(b), T::I(b), T::I(b)], ret.clone(), &core);
                for _ in 0..3 {
                    let (x, y, z) = (pick(rng, ty), pick(rng, ty), pick(rng, ty));
                    s.case(f, vec![x.clone(), x, z.clone()]);
                    s.case(f, vec![y, pick(rng, ty), z]);
                }
            }
        }
    }
}

/// Every fence, around an addition.
fn add_fences(s: &mut Suite, rng: &mut Rng) {
    for order in ["acquire", "release", "acq_rel", "seq_cst"] {
        for scope in ["", "syncscope(\"singlethread\") "] {
            let core = format!(
                "  fence {scope}{order}\n  %s = add i32 %a, %b\n  fence {scope}{order}\n  \
                 %r = xor i32 %s, %a\n"
            );
            let f = s.add(&[T::I(32), T::I(32)], T::I(32), &core);
            s.random_cases(rng, f, 2);
        }
    }
}

/// The whole single-threaded table (one register wide).
fn table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for bits in WIDTHS {
        add_rmw(&mut s, rng, Ty::Int(bits), &LLVM19_OPS, 5);
    }
    for ty in [Ty::Float, Ty::Double] {
        add_rmw(&mut s, rng, ty, &["xchg", "fadd", "fsub"], 5);
    }
    for ty in [Ty::Half, Ty::BFloat, Ty::Ptr] {
        add_rmw(&mut s, rng, ty, &["xchg"], 5);
    }
    let all = WIDTHS.map(Ty::Int).into_iter().chain([Ty::Ptr]);
    for ty in all.clone().chain([Ty::Half, Ty::BFloat, Ty::Float, Ty::Double]) {
        add_load_store(&mut s, rng, ty);
    }
    for ty in all {
        add_cmpxchg(&mut s, rng, ty);
    }
    add_fences(&mut s, rng);
    // Constant operands: an immediate (or a constant materialised into a
    // register) in place of a register; the result used and unused.
    for (bits, op, k) in [
        (32, "add", "7"),
        (64, "sub", "-1"),
        (8, "and", "15"),
        (16, "or", "-32768"),
        (64, "xchg", "81985529216486895"),
        (32, "max", "-5"),
        (8, "uinc_wrap", "200"),
        (64, "add", "4294967296"),
    ] {
        let a = bits / 8;
        let core = format!(
            "  %p = alloca i{bits}, align {a}\n  store i{bits} %a, ptr %p\n  \
             %old = atomicrmw {op} ptr %p, i{bits} {k} seq_cst, align {a}\n  \
             %new = load i{bits}, ptr %p\n  %r = xor i{bits} %old, %new\n"
        );
        let f = s.add(&[T::I(bits)], T::I(bits), &core);
        s.random_cases(rng, f, 3);
        let core = format!(
            "  %p = alloca i{bits}, align {a}\n  store i{bits} %a, ptr %p\n  \
             %old = atomicrmw {op} ptr %p, i{bits} {k} monotonic, align {a}\n  \
             %r = load i{bits}, ptr %p\n"
        );
        let f = s.add(&[T::I(bits)], T::I(bits), &core);
        s.random_cases(rng, f, 3);
    }
    // The `lock <op>` forms: the result unused, every width and order.
    for bits in WIDTHS {
        for op in ["add", "sub", "and", "or", "xor"] {
            for order in ["monotonic", "seq_cst"] {
                let a = bits / 8;
                let core = format!(
                    "  %p = alloca i{bits}, align {a}\n  store i{bits} %a, ptr %p\n  \
                     %old = atomicrmw {op} ptr %p, i{bits} %b {order}, align {a}\n  \
                     %r = load i{bits}, ptr %p\n"
                );
                let f = s.add(&[T::I(bits), T::I(bits)], T::I(bits), &core);
                s.random_cases(rng, f, 3);
            }
        }
    }
    // A `cmpxchg` with constant operands, and with only the flag used.
    for bits in WIDTHS {
        let a = bits / 8;
        let core = format!(
            "  %p = alloca i{bits}, align {a}\n  store i{bits} %a, ptr %p\n  \
             %x = cmpxchg ptr %p, i{bits} 0, i{bits} -1 seq_cst seq_cst, align {a}\n  \
             %ok = extractvalue {{ i{bits}, i1 }} %x, 1\n  \
             %v = load i{bits}, ptr %p\n  %z = zext i1 %ok to i{bits}\n  %r = add i{bits} %v, %z\n"
        );
        let f = s.add(&[T::I(bits)], T::I(bits), &core);
        s.case(f, vec![vec![0; a as usize]]);
        s.random_cases(rng, f, 2);
    }
    // Edges of the wrapping, saturating and ordered operations.
    for bits in WIDTHS {
        for op in ["uinc_wrap", "udec_wrap", "max", "min", "umax", "umin"] {
            let a = bits / 8;
            let core = format!(
                "  %p = alloca i{bits}, align {a}\n  store i{bits} %a, ptr %p\n  \
                 %old = atomicrmw {op} ptr %p, i{bits} %b seq_cst, align {a}\n  \
                 %r = load i{bits}, ptr %p\n"
            );
            let f = s.add(&[T::I(bits), T::I(bits)], T::I(bits), &core);
            let e = edges(bits);
            for x in e.iter().take(8) {
                for y in e.iter().take(8) {
                    s.case(f, vec![bytes_of(x, bits), bytes_of(y, bits)]);
                }
            }
        }
    }
    s
}

#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn summary(name: &str, suite: &Suite, report: &Report) {
    eprintln!(
        "x64 atomic {name}: {} functions, {} cases: {} against the interpreter, {} against llc \
         only, {} undefined (not compared); legs {:?}; {} IR instructions compiled in {:?} \
         ({:.0} ns each); text hod {} B, llc 22 {} B (ratio {:.2}); tools {:?}",
        suite.funcs.len(),
        suite.cases.len(),
        report.interp,
        report.llc_only,
        report.undefined,
        report.legs,
        report.insts,
        report.compile,
        report.compile.as_nanos() as f64 / report.insts.max(1) as f64,
        report.text.0,
        report.text.1,
        report.text.0 as f64 / report.text.1.max(1) as f64,
        report.tools,
    );
}

/// Every operation, width and ordering up to 8 bytes, pointers and the
/// floating formats, against the interpreter and `llc` (plain, every
/// stress option, mixed objects).
#[test]
fn every_atomic_operation_agrees() {
    let suite = table(&mut Rng(133));
    let report = run("atomic", &suite);
    summary("table", &suite, &report);
    assert!(report.interp > suite.cases.len() / 2, "the interpreter decided too few");
    assert_agrees(&report);
}

/// 16 bytes with `+cx16`: every operation, ordering and `cmpxchg` form
/// on a runner whose CPU model has `cmpxchg16b`.
#[test]
fn every_16_byte_atomic_agrees_with_cx16() {
    atomic_wide::check();
}

/// Contended programs on real threads (`common/atomic_threads.rs`).
#[test]
fn contended_programs_on_threads_agree() {
    atomic_threads::check();
}

// ---- the instruction forms ----

/// The disassembly of `@f` alone (`llvm-objdump`), or `None` without it.
fn disassemble(name: &str, functions: &str) -> Option<String> {
    let objdump = hod_testutil::tool("llvm-objdump")?;
    let (module, bodies) = parse(&format!("{}{functions}{PIE_FLAGS}", header()));
    let bytes = hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default()))
        .unwrap_or_else(|e| panic!("{e}\n{functions}"));
    let scratch = Scratch::new(&format!("x64-atomic-forms-{name}"));
    let object = scratch.write("f.o", bytes);
    let mut command = Command::new(objdump);
    command.args(["-d", "--no-show-raw-insn", "-M", "intel"]).arg(&object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    Some(outcome.stdout_text())
}

/// The instructions of the disassembly, one string each, spaces folded
/// (`llvm-objdump` prints a `lock` prefix on its own line: joined to its
/// instruction).
fn instructions(listing: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in listing.lines() {
        let Some((at, rest)) = line.trim_start().split_once(':') else { continue };
        if u64::from_str_radix(at, 16).is_err() {
            continue;
        }
        let inst = rest.split_whitespace().collect::<Vec<_>>().join(" ");
        match out.last_mut() {
            Some(last) if last == "lock" => *last = format!("lock {inst}"),
            _ => out.push(inst),
        }
    }
    out
}

/// The forms `spec/codegen.md` §5.6.10 names, by disassembly: `xchg`
/// for a `seq_cst` store and a plain `mov` for the others; `lock xadd`,
/// `lock <op>` when the result is unused, a `lock cmpxchg` loop for the
/// operations x86 has no instruction for; `mfence` only for a
/// system-scope `seq_cst` fence; `lock cmpxchg16b` with `+cx16`.
#[test]
fn the_instruction_forms_are_the_documented_ones() {
    let f = |body: &str| format!("define i64 @f(ptr %p, i64 %v) {{\nentry:\n{body}\n}}\n");
    let cases: [(&str, String, &[&str], &[&str]); 12] = [
        (
            "store-seq-cst",
            f("  store atomic i64 %v, ptr %p seq_cst, align 8\n  ret i64 0"),
            &["xchg qword ptr"],
            &["mfence"],
        ),
        (
            "store-release",
            f("  store atomic i32 7, ptr %p release, align 4\n  ret i64 0"),
            &["mov dword ptr"],
            &["xchg", "mfence", "lock"],
        ),
        (
            "load-seq-cst",
            f("  %x = load atomic i16, ptr %p seq_cst, align 2\n  %r = zext i16 %x to i64\n  ret i64 %r"),
            &["movzx"],
            &["lock", "mfence"],
        ),
        (
            "rmw-add",
            f("  %x = atomicrmw add ptr %p, i64 %v seq_cst, align 8\n  ret i64 %x"),
            &["lock xadd qword ptr"],
            &["cmpxchg"],
        ),
        (
            "rmw-sub",
            f("  %x = atomicrmw sub ptr %p, i64 %v acquire, align 8\n  ret i64 %x"),
            &["neg", "lock xadd qword ptr"],
            &["cmpxchg"],
        ),
        (
            "rmw-dead-or",
            f("  %x = atomicrmw or ptr %p, i64 %v monotonic, align 8\n  ret i64 0"),
            &["lock or qword ptr"],
            &["cmpxchg", "xadd"],
        ),
        (
            "rmw-xchg",
            f("  %t = trunc i64 %v to i8\n  %x = atomicrmw xchg ptr %p, i8 %t release, align 1\n  %r = zext i8 %x to i64\n  ret i64 %r"),
            &["xchg byte ptr"],
            &["lock", "cmpxchg"],
        ),
        (
            "rmw-umax",
            f("  %x = atomicrmw umax ptr %p, i64 %v seq_cst, align 8\n  ret i64 %x"),
            &["lock cmpxchg qword ptr", "cmovb"],
            &[],
        ),
        (
            "cmpxchg",
            f("  %x = cmpxchg ptr %p, i64 0, i64 %v acq_rel acquire, align 8\n  %o = extractvalue { i64, i1 } %x, 0\n  ret i64 %o"),
            &["lock cmpxchg qword ptr", "sete"],
            &[],
        ),
        (
            "fence-seq-cst",
            f("  fence seq_cst\n  ret i64 0"),
            &["mfence"],
            &[],
        ),
        (
            "fences-free",
            f("  fence acquire\n  fence release\n  fence acq_rel\n  fence syncscope(\"singlethread\") seq_cst\n  ret i64 0"),
            &[],
            &["mfence", "lock"],
        ),
        (
            "cx16",
            "define i64 @f(ptr %p, i64 %v) #0 {\nentry:\n  %x = load atomic i128, ptr %p acquire, align 16\n  %t = trunc i128 %x to i64\n  ret i64 %t\n}\nattributes #0 = { \"target-features\"=\"+cx16\" }\n".to_string(),
            &["lock cmpxchg16b xmmword ptr"],
            &["call"],
        ),
    ];
    for (name, text, present, absent) in cases {
        let Some(listing) = disassemble(name, &text) else { return };
        let insts = instructions(&listing);
        for want in present {
            assert!(
                insts.iter().any(|i| i.starts_with(want) || i.contains(want)),
                "{name}: no `{want}` in\n{listing}"
            );
        }
        for not in absent {
            assert!(!insts.iter().any(|i| i.contains(not)), "{name}: `{not}` in\n{listing}");
        }
    }
}

// ---- refusals ----

/// What compiling `.ll` functions (the header added) reports.
fn compile_ll(functions: &str) -> Result<(), CgError> {
    let (module, bodies) = parse(&format!("{}{functions}{PIE_FLAGS}", header()));
    match hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default())) {
        Ok(_) => Ok(()),
        Err(hod_x64::ModuleError::Function { error, .. }) => Err(error),
        Err(other) => panic!("{other}"),
    }
}

/// What hod refuses: an under-aligned access, a type with no atomic
/// sequence, a `bfloat` arithmetic operation, the floating operations
/// that wait for WP-127's arithmetic, 16 bytes without `+cx16` (no
/// `libatomic` call yet).
#[test]
fn refused_atomics_by_name() {
    let f = |body: &str| format!("define void @f(ptr %p) {{\nentry:\n  {body}\n  ret void\n}}\n");
    let cases = [
        (f("%v = load atomic i32, ptr %p seq_cst, align 2"), atomic::UNALIGNED),
        (f("store atomic i64 0, ptr %p release, align 4"), atomic::UNALIGNED),
        (f("%v = atomicrmw add ptr %p, i32 1 seq_cst, align 1"), atomic::UNALIGNED),
        (f("%v = cmpxchg ptr %p, i64 0, i64 1 seq_cst seq_cst, align 4"), atomic::UNALIGNED),
        (f("%v = load atomic fp128, ptr %p seq_cst, align 16"), atomic::TYPE),
        (f("%v = atomicrmw fadd ptr %p, bfloat 1.0 seq_cst"), atomic::BFLOAT_OP),
        // `fmax` .. `fminimum`, `half` arithmetic: WP-144 (`tests/leftovers.rs`).
        (f("%v = atomicrmw fmax ptr %p, bfloat 1.0 seq_cst"), atomic::BFLOAT_OP),
        (f("%v = load atomic i128, ptr %p seq_cst, align 16"), atomic::LIBATOMIC),
        (f("store atomic i128 0, ptr %p release, align 16"), atomic::LIBATOMIC),
        (f("%v = atomicrmw add ptr %p, i128 1 seq_cst, align 16"), atomic::LIBATOMIC),
        (f("%v = cmpxchg ptr %p, i128 0, i128 1 seq_cst seq_cst, align 16"), atomic::LIBATOMIC),
    ];
    for (text, reason) in &cases {
        assert_eq!(compile_ll(text), Err(CgError::unsupported(reason)), "{text}");
    }
    // `-cx16` after `+cx16` turns it off again; the last one wins.
    let off = "define void @f(ptr %p) #0 {\nentry:\n  %v = load atomic i128, ptr %p seq_cst, align 16\n  ret void\n}\nattributes #0 = { \"target-features\"=\"+cx16,-cx16\" }\n";
    assert_eq!(compile_ll(off), Err(CgError::unsupported(atomic::LIBATOMIC)));
    let on = off.replace("+cx16,-cx16", "-cx16,+sse2,+cx16");
    assert_eq!(compile_ll(&on), Ok(()));
}
