//! Atomics (WP-074, `spec/codegen.md` §5.5.1 "Atomics"): every atomic
//! operation at every width and ordering, single-threaded, checked
//! natively against `hod_interp` and `llc -O0 -global-isel=0
//! -fast-isel=0` under both sequences (LL/SC loops, and the outline
//! helpers of `+outline-atomics`); that no memory access or call sits
//! between an exclusive load and its store; contended programs on real
//! threads (`common/atomic_threads.rs`); what is refused, by name.

#![allow(clippy::many_single_char_names, reason = "`.ll` text built by hand")]

mod common;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;

#[path = "common/atomic_threads.rs"]
mod atomic_threads;

use std::fmt::Write as _;

use aggr_gen::{Report, Rng, Suite, T, assert_agrees, bytes_of, edges, run, value};
use common::{assert_refused, code_words, ll_header, parse};
use hod_a64::atomic;
use hod_cg::{Options, Stress};

/// The orderings an `atomicrmw` or a `cmpxchg` success takes.
const RMW_ORDERS: [&str; 5] = ["monotonic", "acquire", "release", "acq_rel", "seq_cst"];
/// The orderings a `cmpxchg` failure takes.
const FAIL_ORDERS: [&str; 3] = ["monotonic", "acquire", "seq_cst"];
/// The integer operations.
const INT_OPS: [&str; 15] = [
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
/// The floating-point operations.
const FLOAT_OPS: [&str; 7] = ["xchg", "fadd", "fsub", "fmax", "fmin", "fmaximum", "fminimum"];
/// The integer widths.
const WIDTHS: [u32; 5] = [8, 16, 32, 64, 128];

/// A type an atomic operation works on, carried in the suite as the
/// integer of its bits (the interpreter's twins take integers only).
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

    /// `%<name>` of this type from the integer `%<from>`.
    fn of_int(self, name: &str, from: &str) -> String {
        let b = self.bits();
        match self {
            Ty::Int(_) => format!("  %{name} = add i{b} %{from}, 0\n"),
            Ty::Ptr => format!("  %{name} = inttoptr i64 %{from} to ptr\n"),
            _ => format!("  %{name} = bitcast i{b} %{from} to {}\n", self.ll()),
        }
    }

    /// The integer `%<name>` from `%<from>` of this type.
    fn to_int(self, name: &str, from: &str) -> String {
        let b = self.bits();
        match self {
            Ty::Int(_) => format!("  %{name} = add i{b} %{from}, 0\n"),
            Ty::Ptr => format!("  %{name} = ptrtoint ptr %{from} to i64\n"),
            _ => format!("  %{name} = bitcast {} %{from} to i{b}\n", self.ll()),
        }
    }
}

/// Bit patterns worth trying for a floating-point format of `bits`
/// (zeros, ones, fractions, infinities, a NaN, the largest finite).
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

/// The `{ old, new }` result type of an operation on `ty`.
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

/// The whole single-threaded table.
fn table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for bits in WIDTHS {
        add_rmw(&mut s, rng, Ty::Int(bits), &INT_OPS, 5);
    }
    for ty in [Ty::Half, Ty::Float, Ty::Double] {
        add_rmw(&mut s, rng, ty, &FLOAT_OPS, 5);
    }
    add_rmw(&mut s, rng, Ty::BFloat, &["xchg"], 5);
    add_rmw(&mut s, rng, Ty::Ptr, &["xchg"], 5);
    let all = WIDTHS.map(Ty::Int).into_iter().chain([Ty::Ptr]);
    for ty in all.clone().chain([Ty::Half, Ty::BFloat, Ty::Float, Ty::Double]) {
        add_load_store(&mut s, rng, ty);
    }
    for ty in all {
        add_cmpxchg(&mut s, rng, ty);
    }
    add_fences(&mut s, rng);
    // Constant operands: an immediate in place of a register.
    for (bits, op, k) in [(32, "add", "7"), (64, "sub", "-1"), (8, "and", "15"), (128, "or", "1")] {
        let core = format!(
            "  %p = alloca i{bits}, align {a}\n  store i{bits} %a, ptr %p\n  \
             %old = atomicrmw {op} ptr %p, i{bits} {k} seq_cst, align {a}\n  \
             %new = load i{bits}, ptr %p\n  %r = xor i{bits} %old, %new\n",
            a = bits / 8
        );
        let f = s.add(&[T::I(bits)], T::I(bits), &core);
        s.random_cases(rng, f, 3);
    }
    // Edges of the wrapping and saturating operations at every width.
    for bits in WIDTHS {
        for op in ["uinc_wrap", "udec_wrap", "usub_cond", "usub_sat", "max", "umin"] {
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

/// The suite with every function asking for outline atomics.
fn outlined(mut s: Suite) -> Suite {
    for text in [&mut s.text, &mut s.twins, &mut s.reg_twins] {
        *text = text.replace(") {\nentry:", ") #0 {\nentry:");
    }
    s.decls.insert("attributes #0 = { \"target-features\"=\"+outline-atomics,+v8a\" }".into());
    s
}

#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn summary(name: &str, suite: &Suite, report: &Report) {
    eprintln!(
        "a64 atomic {name}: {} functions, {} cases: {} against the interpreter, {} against llc \
         only, {} undefined (not compared); {} IR instructions compiled in {:?} ({:.0} ns each); \
         tools {:?}",
        suite.funcs.len(),
        suite.cases.len(),
        report.interp,
        report.llc_only,
        report.undefined,
        report.insts,
        report.compile,
        report.compile.as_nanos() as f64 / report.insts.max(1) as f64,
        report.tools,
    );
}

fn check(name: &str, suite: &Suite) {
    let report = run(name, suite);
    summary(name, suite, &report);
    assert!(report.interp > suite.cases.len() / 2, "{name}: the interpreter decided too few");
    assert_agrees(&report);
}

/// Every operation, width and ordering through LL/SC loops (no
/// `target-features`).
#[test]
fn every_atomic_operation_agrees_with_exclusive_loops() {
    check("llsc", &table(&mut Rng(74)));
}

/// The same table under `+outline-atomics`: the `__aarch64_*` helpers
/// where LLVM calls them, LL/SC loops elsewhere.
#[test]
fn every_atomic_operation_agrees_with_outline_helpers() {
    check("outline", &outlined(table(&mut Rng(74))));
}

/// The kind of an instruction word, for the exclusive-window scan.
#[derive(Debug, PartialEq, Eq)]
enum Word {
    /// `ldxr`, `ldaxr`, `ldxp`, `ldaxp`.
    LoadExclusive,
    /// `stxr`, `stlxr`, `stxp`, `stlxp`.
    StoreExclusive,
    /// `clrex`.
    Clrex,
    /// Any other load or store (integer or SIMD, acquire/release, LSE).
    Access,
    /// `bl`, `blr`.
    Call,
    Other,
}

fn word(w: u32) -> Word {
    if w & 0x3f00_0000 == 0x0800_0000 && w & (1 << 23) == 0 {
        return if w & (1 << 22) != 0 { Word::LoadExclusive } else { Word::StoreExclusive };
    }
    if w & 0xffff_f0ff == 0xd503_305f {
        return Word::Clrex;
    }
    if w & 0xfc00_0000 == 0x9400_0000 || w & 0xffff_fc1f == 0xd63f_0000 {
        return Word::Call;
    }
    if w & (1 << 27) != 0 && w & (1 << 25) == 0 {
        return Word::Access;
    }
    Word::Other
}

/// Scans `words` in layout order: between an exclusive load and the next
/// exclusive store (or `clrex`) there is no other memory access and no
/// call. Returns how many exclusive windows there are.
fn scan(words: &[u32]) -> Result<usize, String> {
    let (mut open, mut windows) = (None, 0);
    for (i, &w) in words.iter().enumerate() {
        match (word(w), open) {
            (Word::LoadExclusive, None) => open = Some(i),
            (Word::StoreExclusive | Word::Clrex, Some(_)) => {
                open = None;
                windows += 1;
            }
            (Word::Other, _)
            | (Word::StoreExclusive | Word::Clrex | Word::Access | Word::Call, None) => {}
            (kind, Some(at)) => {
                return Err(format!("{kind:?} {w:#010x} at word {i} inside the window from {at}"));
            }
        }
    }
    match open {
        None => Ok(windows),
        Some(at) => Err(format!("the window from word {at} never closes")),
    }
}

/// The spill policy's guarantee (`spec/codegen.md` §5.5.1 "Atomics"):
/// every operand is in a register before an exclusive loop starts, so no
/// glue (spill, reload, rematerialised address) lands inside it, even
/// when every value is forced through memory.
#[test]
fn no_memory_access_between_exclusive_load_and_store() {
    let s = table(&mut Rng(74));
    for (name, suite) in [("llsc", &s), ("outline", &outlined(table(&mut Rng(74))))] {
        let text = suite.module(&ll_header(22));
        let (module, bodies) = parse(&text);
        for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
            let words = code_words(&module, &bodies, &Options { stress, ..Options::default() });
            let windows = scan(&words).unwrap_or_else(|e| panic!("{name} {mode}: {e}"));
            eprintln!("a64 atomic {name} {mode}: {windows} exclusive windows, all clean");
            assert!(windows > 300, "{name} {mode}: only {windows} exclusive windows");
        }
    }
}

/// The scan itself catches a store inside a window.
#[test]
fn the_window_scan_catches_an_access() {
    // ldaxr w9, [x10]; str w9, [sp]; stlxr w17, w9, [x10]
    assert!(scan(&[0x885f_fd49, 0xb900_03e9, 0x8811_fd49]).is_err());
    assert_eq!(scan(&[0x885f_fd49, 0x0b09_0129, 0x8811_fd49]), Ok(1));
}

/// Under `+outline-atomics`, `cmpxchg` and the operations LLVM gives a
/// helper call the helper by its libgcc name; without it there is no call.
#[test]
fn outline_helpers_are_called_where_llvm_calls_them() {
    let f = |attr: &str, inst: &str| {
        format!(
            "define i32 @f(ptr %p, i32 %v) {attr}{{\nentry:\n  {inst}\n  ret i32 0\n}}\n\
             attributes #0 = {{ \"target-features\"=\"+outline-atomics\" }}\n"
        )
    };
    for (inst, helper) in [
        ("%x = atomicrmw add ptr %p, i32 %v seq_cst", "__aarch64_ldadd4_acq_rel"),
        ("%x = atomicrmw sub ptr %p, i32 %v acquire", "__aarch64_ldadd4_acq"),
        ("%x = atomicrmw and ptr %p, i32 %v release", "__aarch64_ldclr4_rel"),
        ("%x = atomicrmw xchg ptr %p, i32 %v monotonic", "__aarch64_swp4_relax"),
        ("%x = cmpxchg ptr %p, i32 0, i32 %v acquire monotonic", "__aarch64_cas4_acq"),
        ("%x = load atomic i128, ptr %p seq_cst, align 16", "__aarch64_cas16_acq_rel"),
    ] {
        for (attr, called) in [("#0 ", true), ("", false)] {
            let (m, bodies) = parse(&format!("{}{}", ll_header(22), f(attr, inst)));
            let emitted = common::emit(&m, &bodies, &Options::default());
            let elf = hod_obj::elf::to_vec(&emitted.object).unwrap();
            let name = format!("\0{helper}\0");
            let found = elf.windows(name.len()).any(|w| w == name.as_bytes());
            assert_eq!(found, called, "{inst} {attr}");
        }
    }
}

/// What hod refuses: an under-aligned access, a type with no atomic
/// sequence, a `bfloat` arithmetic operation.
#[test]
fn refused_atomics_by_name() {
    let f = |body: &str| format!("define void @f(ptr %p) {{\nentry:\n  {body}\n  ret void\n}}\n");
    assert_refused(&[
        (&f("%v = load atomic i32, ptr %p seq_cst, align 2"), atomic::UNALIGNED),
        (&f("store atomic i64 0, ptr %p release, align 4"), atomic::UNALIGNED),
        (&f("%v = atomicrmw add ptr %p, i32 1 seq_cst, align 1"), atomic::UNALIGNED),
        (&f("%v = cmpxchg ptr %p, i128 0, i128 1 seq_cst seq_cst, align 8"), atomic::UNALIGNED),
        (&f("%v = load atomic fp128, ptr %p seq_cst, align 16"), atomic::TYPE),
        (&f("%v = atomicrmw fadd ptr %p, bfloat 1.0 seq_cst"), atomic::BFLOAT_OP),
    ]);
}

/// Contended programs on real threads (`common/atomic_threads.rs`):
/// every read-modify-write operation as a shared counter, spinlocks on
/// strong and weak `cmpxchg` and on exchange, a Treiber stack, 128-bit
/// exchange and loads that must not tear, release/acquire message
/// passing; LL/SC and outline builds, every linker, against a reference
/// built by gcc.
#[test]
fn contended_programs_on_threads_agree() {
    atomic_threads::check();
}

/// Found by the corpus run (`gcc-torture/pr79286.c`), not an atomic: a
/// load through a constant `getelementptr` far beyond its global (an
/// out-of-bounds index LLVM folds into the address) gets an `adrp` of
/// `.bss` plus the offset, which no linker can relocate
/// ("relocation truncated to fit: `R_AARCH64_ADR_PREL_PG_HI21`"); `llc`'s
/// object links. The address computation is `mem.rs`'s (WP-036) /
/// constants with addresses (WP-072).
#[test]
#[ignore = "a bug outside WP-074: a far constant offset from a symbol does not link"]
fn a_far_constant_offset_from_a_global_links() {
    let text = format!(
        "{}@d = internal global [0 x [8 x i32]] zeroinitializer, align 4\n\
         define i32 @main() {{\nentry:\n  %v = load volatile i32, ptr getelementptr inbounds \
         ([0 x [8 x i32]], ptr @d, i64 0, i64 300000000000000000), align 4\n  ret i32 0\n}}\n",
        ll_header(22)
    );
    let (m, bodies) = parse(&text);
    let emitted = common::emit(&m, &bodies, &Options::default());
    let scratch = hod_testutil::Scratch::new("a64-atomic-far-offset");
    let object = common::write_object(&scratch, "far.o", &emitted);
    let Some(toolchain) = common::linking::Toolchain::host(&scratch) else { return };
    for linker in &toolchain.linkers {
        let outcome = toolchain.link(linker, &[], &[&object], &scratch.join(linker.name));
        assert!(outcome.success(), "{}: {}", linker.name, outcome.stderr_text());
    }
}
