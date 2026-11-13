//! The short tail, second part (WP-095, `spec/codegen.md` §5.5.1
//! "WP-095"): scalar `bfloat`, `extractelement` of `i128` lanes, label
//! differences in initializers, `bitcast` of a constant vector to a
//! scalar, block addresses taken only by an unreachable `indirectbr`,
//! `preserve_mostcc`, `llvm.global.annotations` and `"fmv-features"`.
//!
//! Each table of functions is compiled by hod (plain, every stress
//! option, far mode) and by `llc -O0 -global-isel=0 -fast-isel=0`, linked
//! with a C driver under every linker present and run
//! (`common::native`). The reference is `hod_interp` where it models the
//! construct, else the `llc` build.

mod common;
#[path = "common/native.rs"]
mod native;

use std::fmt::Write as _;

use common::linking::Toolchain;
use common::{assert_refused, compile_ll, ll_header, llc, llc_major, objdump, parse, write_object};
use hod_a64::{compile_module, tail2};
use hod_cg::{Options, RelocModel, Stress};
use hod_interp::{Interp, Program, Val};

const DEFAULT: Options = Options {
    reloc_model: RelocModel::Static,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false, coro_frame: false },
    debug_compression: hod_obj::Compression::None,
    split_dwarf: hod_dwarf::SplitDwarf::Off,
};

const MODES: [(&str, Options); 3] = [
    ("plain", DEFAULT),
    ("stress", Options { stress: Stress::ALL, ..DEFAULT }),
    (
        "far",
        Options {
            stress: Stress { min_pool: false, far: true, displace: false, coro_frame: false },
            ..DEFAULT
        },
    ),
];

/// xorshift64*: a fixed sequence, so that every run checks the same calls.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// An edge value or a random word, shifted so that every magnitude
    /// comes up.
    fn word(&mut self) -> u64 {
        const EDGES: [u64; 12] = [
            0,
            1,
            u64::MAX,
            1 << 63,
            (1 << 63) - 1,
            0xffff_ffff,
            0x7f80_0000,
            0xff80_0001,
            0x7fc0_1234,
            0x3f80_8000,
            0x7ff0_0000_0000_0001,
            0x3ff0_0000_0000_0001,
        ];
        let r = self.next();
        if r.is_multiple_of(4) {
            EDGES[(r >> 8) as usize % EDGES.len()]
        } else {
            self.next() >> (r % 64)
        }
    }
}

/// `count` calls of `name` with words from `rng`.
fn calls_of(name: &str, rng: &mut Rng, count: usize) -> Vec<native::Call> {
    (0..count).map(|_| (name.to_owned(), [(); 6].map(|()| rng.word()))).collect()
}

/// Runs `calls` of `body` natively against `want` (or `llc`).
fn run(label: &str, body: &str, calls: &[native::Call], want: Option<&str>) -> usize {
    let count = native::run_table(label, body, calls, &MODES, want);
    eprintln!("{label}: {count} calls agree");
    count
}

/// What `hod_interp` prints for each call of `body`'s functions.
fn interp_lines(body: &str, calls: &[native::Call]) -> String {
    let text = format!("{}{body}", ll_header(22));
    let (module, bodies) = parse(&text);
    let program = Program::with_bodies(&module, &bodies).expect("a program");
    let mut lines = String::new();
    for (name, args) in calls {
        let sym = module.find_symbol(name.as_bytes()).expect("a function");
        let fb = bodies.iter().find(|b| b.symbol() == sym).expect("a body");
        let vals: Vec<Val> =
            args[..fb.arg_count() as usize].iter().map(|&a| Val::int(64, u128::from(a))).collect();
        let outcome = Interp::new(&program).run(sym, &vals);
        let Ok(Some(Val::Int { value, .. })) = outcome.result else {
            panic!("the interpreter on @{name}{args:?}: {:?}", outcome.result);
        };
        writeln!(lines, "{}", value as u64).unwrap();
    }
    lines
}

// ---- bfloat ----

/// `%x`, `%y`: `bfloat`s from the low 16 bits of `%a` and `%b`.
const BF_ARGS: &str = "
  %a16 = trunc i64 %a to i16
  %b16 = trunc i64 %b to i16
  %x = bitcast i16 %a16 to bfloat
  %y = bitcast i16 %b16 to bfloat
";

fn bfloat_body() -> String {
    let mut body = String::from(
        "
define i64 @bf_from_float(i64 %a) {
  %t = trunc i64 %a to i32
  %f = bitcast i32 %t to float
  %h = fptrunc float %f to bfloat
  %i = bitcast bfloat %h to i16
  %r = zext i16 %i to i64
  ret i64 %r
}

define i64 @bf_from_double(i64 %a) {
  %d = bitcast i64 %a to double
  %h = fptrunc double %d to bfloat
  %i = bitcast bfloat %h to i16
  %r = zext i16 %i to i64
  ret i64 %r
}

define i64 @bf_widen(i64 %a) {
  %t = trunc i64 %a to i16
  %h = bitcast i16 %t to bfloat
  %f = fpext bfloat %h to float
  %d = fpext bfloat %h to double
  %fi = bitcast float %f to i32
  %fz = zext i32 %fi to i64
  %di = bitcast double %d to i64
  %r = xor i64 %di, %fz
  ret i64 %r
}
",
    );
    // The arithmetic, four results of 16 bits packed.
    write!(body, "\ndefine i64 @bf_arith(i64 %a, i64 %b) {{{BF_ARGS}").unwrap();
    for (k, op) in ["fadd", "fsub", "fmul", "fdiv"].iter().enumerate() {
        writeln!(
            body,
            "  %r{k} = {op} bfloat %x, %y\n  %i{k} = bitcast bfloat %r{k} to i16\n  \
             %z{k} = zext i16 %i{k} to i64\n  %s{k} = shl i64 %z{k}, {}",
            16 * k
        )
        .unwrap();
    }
    body += "  %o1 = or i64 %s0, %s1\n  %o2 = or i64 %o1, %s2\n  %o3 = or i64 %o2, %s3\n  \
             ret i64 %o3\n}\n";
    write!(
        body,
        "\ndefine i64 @bf_neg(i64 %a, i64 %b) {{{BF_ARGS}  %n = fneg bfloat %x
  %sq = fmul bfloat %x, %y
  %ni = bitcast bfloat %n to i16
  %qi = bitcast bfloat %sq to i16
  %nz = zext i16 %ni to i64
  %qz = zext i16 %qi to i64
  %qs = shl i64 %qz, 32
  %o = or i64 %nz, %qs
  ret i64 %o
}}
"
    )
    .unwrap();
    // Every predicate, one bit each.
    write!(body, "\ndefine i64 @bf_cmp(i64 %a, i64 %b) {{{BF_ARGS}").unwrap();
    let preds = [
        "false", "oeq", "ogt", "oge", "olt", "ole", "one", "ord", "ueq", "ugt", "uge", "ult",
        "ule", "une", "uno", "true",
    ];
    let mut acc = String::from("0");
    for (k, pred) in preds.iter().enumerate() {
        writeln!(
            body,
            "  %c{k} = fcmp {pred} bfloat %x, %y\n  %cz{k} = zext i1 %c{k} to i64\n  \
             %cs{k} = shl i64 %cz{k}, {k}\n  %ca{k} = or i64 {acc}, %cs{k}"
        )
        .unwrap();
        acc = format!("%ca{k}");
    }
    writeln!(body, "  ret i64 {acc}\n}}").unwrap();
    body += &bfloat_int_body();
    body
}

/// The conversions between `bfloat` and integers.
fn bfloat_int_body() -> String {
    let mut body = String::new();
    // From integers of each width, signed and unsigned.
    for (name, op) in [("bf_sitofp", "sitofp"), ("bf_uitofp", "uitofp")] {
        writeln!(body, "\ndefine i64 @{name}(i64 %a) {{").unwrap();
        let mut acc = String::from("0");
        for (k, bits) in [8, 16, 32, 64].iter().enumerate() {
            let src = if *bits == 64 {
                "%a".to_owned()
            } else {
                writeln!(body, "  %t{k} = trunc i64 %a to i{bits}").unwrap();
                format!("%t{k}")
            };
            writeln!(
                body,
                "  %h{k} = {op} i{bits} {src} to bfloat\n  %i{k} = bitcast bfloat %h{k} to i16\n  \
                 %z{k} = zext i16 %i{k} to i64\n  %s{k} = shl i64 %z{k}, {}\n  \
                 %o{k} = or i64 {acc}, %s{k}",
                16 * k
            )
            .unwrap();
            acc = format!("%o{k}");
        }
        writeln!(body, "  ret i64 {acc}\n}}").unwrap();
    }
    // To integers: from values in range (a 16-bit integer rounded to a
    // `bfloat`), so that no conversion is poison.
    body += "
define i64 @bf_toint(i64 %a) {
  %t = trunc i64 %a to i16
  %hs = sitofp i16 %t to bfloat
  %hu = uitofp i16 %t to bfloat
  %s32 = fptosi bfloat %hs to i32
  %s64 = fptosi bfloat %hs to i64
  %u32 = fptoui bfloat %hu to i32
  %u64 = fptoui bfloat %hu to i64
  %zs = zext i32 %s32 to i64
  %zu = zext i32 %u32 to i64
  %su = shl i64 %zu, 32
  %o = or i64 %zs, %su
  %x1 = xor i64 %o, %s64
  %m = mul i64 %u64, 7
  %x2 = xor i64 %x1, %m
  ret i64 %x2
}
";
    body
}

fn bfloat_calls() -> Vec<native::Call> {
    let mut rng = Rng(0x0950_0095);
    let mut calls = Vec::new();
    for name in [
        "bf_from_float",
        "bf_from_double",
        "bf_widen",
        "bf_arith",
        "bf_neg",
        "bf_cmp",
        "bf_sitofp",
        "bf_uitofp",
        "bf_toint",
    ] {
        calls.extend(calls_of(name, &mut rng, 60));
    }
    // Ties, NaNs, infinities and the values around them, as both kinds of
    // operand.
    for bits in [
        0x3f80_8000u64,
        0x3f81_8000,
        0x3f80_7fff,
        0x7f80_0000,
        0xff80_0000,
        0x7f80_0001,
        0x7fbf_ffff,
        0xffc0_0000,
        0x7f7f_ffff,
        0x0000_8000,
        0x8001_8000,
    ] {
        calls.push(("bf_from_float".to_owned(), [bits, 0, 0, 0, 0, 0]));
    }
    for bits in [0x3ff0_1000_0000_0000u64, 0x3ff0_1000_0000_0001, 0x7ff0_0000_0000_0001, 1] {
        calls.push(("bf_from_double".to_owned(), [bits, 0, 0, 0, 0, 0]));
    }
    for (a, b) in [(0x7fc0, 0x3f80), (0x3f80, 0x0000), (0x8000, 0x0000), (0x7f80, 0xff80)] {
        for name in ["bf_arith", "bf_neg", "bf_cmp"] {
            calls.push((name.to_owned(), [a, b, 0, 0, 0, 0]));
        }
    }
    for a in [0x0100_0001u64, 0x00ff_ffff, 1 << 63, (1 << 63) | 1, 0x0080_8000_0000_0001] {
        calls.push(("bf_sitofp".to_owned(), [a, 0, 0, 0, 0, 0]));
        calls.push(("bf_uitofp".to_owned(), [a, 0, 0, 0, 0, 0]));
    }
    calls
}

/// Against `llc` (the interpreter does not model `bfloat`).
#[test]
fn bfloat_agrees_with_llc() {
    run("a64-tail2-bfloat", &bfloat_body(), &bfloat_calls(), None);
}

/// `frem` calls `fmodf`, which is in `libm`: a driver of its own, linked
/// with `-lm`, against `llc`.
#[test]
fn bfloat_frem_agrees_with_llc() {
    let body = format!(
        "define i64 @bf_rem(i64 %a, i64 %b) {{{BF_ARGS}  %r = frem bfloat %x, %y
  %i = bitcast bfloat %r to i16
  %z = zext i16 %i to i64
  ret i64 %z
}}
"
    );
    let mut rng = Rng(0x0953_0095);
    let mut pairs: Vec<(u64, u64)> = (0..60).map(|_| (rng.word(), rng.word())).collect();
    pairs.extend([(0x4120, 0x4040), (0xc120, 0x4040), (0x7fc0, 0x3f80), (0x3f80, 0), (0x7f80, 1)]);
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\nuint64_t bf_rem(uint64_t, uint64_t);\nint main(void) {\n",
    );
    for (a, b) in &pairs {
        writeln!(c, "  printf(\"%llu\\n\", (unsigned long long)bf_rem({a}ULL, {b}ULL));").unwrap();
    }
    c.push_str("  return 0;\n}\n");
    let dir = hod_testutil::Scratch::new("a64-tail2-bfloat-frem");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", &c, &[]);
    let Some(reference) = llc(&dir, "llc", &format!("{}{body}", ll_header(llc_major())), &[])
    else {
        return;
    };
    let (module, bodies) = parse(&format!("{}{body}", ll_header(22)));
    let mut objects = vec![("llc", reference)];
    for (name, options) in &MODES {
        let emitted = compile_module(&module, &bodies, options).expect("compiles");
        objects.push((*name, write_object(&dir, &format!("{name}.o"), &emitted)));
    }
    // The library follows the objects that need it.
    let libm = std::path::Path::new("-lm");
    let mut want: Option<String> = None;
    for linker in &toolchain.linkers {
        for (name, object) in &objects {
            let exe = dir.join(format!("{name}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, object, libm], &exe);
            assert!(ran.success(), "{name} under {}: {}", linker.name, ran.stderr_text());
            let got = ran.stdout_text();
            let want = want.get_or_insert_with(|| got.clone());
            assert_eq!(&got, want, "{name} under {}", linker.name);
        }
    }
}

/// The narrowing from `double` rounds to odd first, by `FCVTXN`, which
/// `tail2` encodes itself: the disassembler names it.
#[test]
fn fcvtxn_is_encoded() {
    let body = "define i16 @f(double %d) {\n  %h = fptrunc double %d to bfloat\n  \
                %i = bitcast bfloat %h to i16\n  ret i16 %i\n}\n";
    let (module, bodies) = parse(&format!("{}{body}", ll_header(22)));
    let dir = hod_testutil::Scratch::new("a64-tail2-fcvtxn");
    let emitted = compile_module(&module, &bodies, &DEFAULT).expect("compiles");
    let object = write_object(&dir, "f.o", &emitted);
    let Some(lines) = objdump(&object) else { return };
    assert!(lines.iter().any(|l| l.starts_with("fcvtxn s0, d0")), "{lines:#?}");
    assert_eq!(tail2::fcvtxn(mc_v(3), mc_v(17)), 0x7e61_6a23);
}

fn mc_v(index: u8) -> hod_mc_a64::VReg {
    hod_mc_a64::VReg::new(index).expect("a vector register")
}

#[test]
fn bfloat_refusals_are_named() {
    assert_refused(&[(
        "define i16 @f(fp128 %a) {\n  %h = fptrunc fp128 %a to bfloat\n  \
         %i = bitcast bfloat %h to i16\n  ret i16 %i\n}\n",
        tail2::BFLOAT_FP128,
    )]);
}

// ---- extractelement of i128 lanes ----

const EXTRACT_I128: &str = "
define i64 @ee(i64 %a, i64 %b, i64 %c, i64 %d, i64 %i) {
  %p = alloca <3 x i128>, align 16
  %az = zext i64 %a to i128
  %bz = zext i64 %b to i128
  %bs = shl i128 %bz, 64
  %x = or i128 %az, %bs
  %cz = zext i64 %c to i128
  %dz = zext i64 %d to i128
  %ds = shl i128 %dz, 64
  %y = or i128 %cz, %ds
  %z = xor i128 %x, %y
  %p1 = getelementptr i8, ptr %p, i64 16
  %p2 = getelementptr i8, ptr %p, i64 32
  store i128 %x, ptr %p
  store i128 %y, ptr %p1
  store i128 %z, ptr %p2
  %v = load <3 x i128>, ptr %p, align 16
  %j = urem i64 %i, 3
  %e = extractelement <3 x i128> %v, i64 %j
  %k = extractelement <3 x i128> %v, i64 1
  %v1 = load <1 x i128>, ptr %p2, align 16
  %l = extractelement <1 x i128> %v1, i64 0
  %m = mul i128 %e, 3
  %s = add i128 %m, %k
  %t = xor i128 %s, %l
  %lo = trunc i128 %t to i64
  %hi128 = lshr i128 %t, 64
  %hi = trunc i128 %hi128 to i64
  %h = mul i64 %hi, -7046029254386353131
  %r = add i64 %h, %lo
  ret i64 %r
}
";

#[test]
fn extractelement_of_i128_lanes_agrees_with_the_interpreter() {
    let mut rng = Rng(0x0951_0095);
    let mut calls = calls_of("ee", &mut rng, 60);
    for i in 0..6 {
        calls.push(("ee".to_owned(), [1, 2, 3, 4, i, 0]));
    }
    let want = interp_lines(EXTRACT_I128, &calls);
    run("a64-tail2-extract-i128", EXTRACT_I128, &calls, Some(&want));
}

// ---- label differences ----

/// A table of label differences of each width `llc` writes, one with a
/// number added, and a jump through it.
const LABELS: &str = "
@tab = internal global [2 x i32] [i32 trunc (i64 sub (i64 ptrtoint (ptr blockaddress(@sel, %one) to i64), i64 ptrtoint (ptr blockaddress(@sel, %base) to i64)) to i32), i32 trunc (i64 sub (i64 ptrtoint (ptr blockaddress(@sel, %two) to i64), i64 ptrtoint (ptr blockaddress(@sel, %base) to i64)) to i32)], align 4
@tab64 = internal global i64 sub (i64 ptrtoint (ptr blockaddress(@sel, %two) to i64), i64 ptrtoint (ptr blockaddress(@sel, %base) to i64)), align 8
@tab16 = internal global i16 trunc (i64 sub (i64 ptrtoint (ptr blockaddress(@sel, %one) to i64), i64 ptrtoint (ptr blockaddress(@sel, %base) to i64)) to i16), align 2
@tabadd = internal global i64 sub (i64 sub (i64 ptrtoint (ptr blockaddress(@sel, %one) to i64), i64 ptrtoint (ptr blockaddress(@sel, %base) to i64)), i64 12), align 8

define i64 @sel(i64 %i) {
entry:
  %j = and i64 %i, 7
  switch i64 %j, label %none [
    i64 1, label %use1
    i64 2, label %use2
    i64 3, label %use64
    i64 4, label %use16
    i64 5, label %useadd
  ]
use1:
  %o1 = load i32, ptr @tab, align 4
  %s1 = sext i32 %o1 to i64
  br label %go
use2:
  %p2 = getelementptr [2 x i32], ptr @tab, i64 0, i64 1
  %o2 = load i32, ptr %p2, align 4
  %s2 = sext i32 %o2 to i64
  br label %go
use64:
  %o3 = load i64, ptr @tab64, align 8
  br label %go
use16:
  %o4 = load i16, ptr @tab16, align 2
  %s4 = sext i16 %o4 to i64
  br label %go
useadd:
  %o5 = load i64, ptr @tabadd, align 8
  %s5 = add i64 %o5, 12
  br label %go
none:
  br label %go
go:
  %off = phi i64 [ %s1, %use1 ], [ %s2, %use2 ], [ %o3, %use64 ], [ %s4, %use16 ], [ %s5, %useadd ], [ 0, %none ]
  %t = getelementptr i8, ptr blockaddress(@sel, %base), i64 %off
  indirectbr ptr %t, [label %base, label %one, label %two]
base:
  ret i64 100
one:
  ret i64 101
two:
  ret i64 102
}
";

#[test]
fn label_differences_agree_with_the_interpreter() {
    let calls: Vec<native::Call> =
        (0..16).map(|i| ("sel".to_owned(), [i, 0, 0, 0, 0, 0])).collect();
    let want = interp_lines(LABELS, &calls);
    assert!(want.lines().any(|l| l == "102"), "the table reaches every block:\n{want}");
    run("a64-tail2-labels", LABELS, &calls, Some(&want));
}

/// The difference is a number in the data: no relocation.
#[test]
fn a_label_difference_has_no_relocation() {
    let (module, bodies) = parse(&format!("{}{LABELS}", ll_header(22)));
    let emitted = compile_module(&module, &bodies, &DEFAULT).expect("compiles");
    let dir = hod_testutil::Scratch::new("a64-tail2-labels-reloc");
    let object = write_object(&dir, "labels.o", &emitted);
    let Some(readelf) = hod_testutil::find_tool("llvm-readelf") else { return };
    let mut command = std::process::Command::new(readelf);
    command.arg("-r").arg(&object);
    let outcome = hod_testutil::run_with(&mut command, &[], std::time::Duration::from_mins(1));
    assert!(outcome.success(), "{}", outcome.stderr_text());
    let text = outcome.stdout_text();
    assert!(!text.contains(".rela.data"), "{text}");
}

/// Shapes that are not a label difference of one function keep their
/// names; one as an operand is `operand:block-diff`.
#[test]
fn other_label_shapes_are_refused_by_name() {
    let classify = |text: &str| {
        let (module, bodies) = parse(&format!("{}{text}", ll_header(22)));
        let module_pass = hod_ir::classify(&module).err();
        let body_pass = bodies.iter().find_map(|b| hod_ir::classify_body(&module, b).err());
        module_pass.or(body_pass).map(|r| r.construct.into_owned())
    };
    let two = "
define void @f(ptr %p) {
entry:
  indirectbr ptr %p, [label %a]
a:
  ret void
}
define void @g(ptr %p) {
entry:
  indirectbr ptr %p, [label %b]
b:
  ret void
}
";
    let across = format!(
        "@x = global i64 sub (i64 ptrtoint (ptr blockaddress(@f, %a) to i64), i64 ptrtoint (ptr blockaddress(@g, %b) to i64))\n{two}"
    );
    assert_eq!(classify(&across).as_deref(), Some("const-expr:sub"));
    let operand = "
define i64 @h(ptr %p) {
entry:
  indirectbr ptr %p, [label %a, label %b]
a:
  ret i64 sub (i64 ptrtoint (ptr blockaddress(@h, %a) to i64), i64 ptrtoint (ptr blockaddress(@h, %b) to i64))
b:
  ret i64 0
}
";
    assert_eq!(classify(operand).as_deref(), Some("operand:block-diff"));
}

// ---- a bitcast of a constant vector to a scalar ----

const VECTOR_BITS: &str = "
define i64 @vbits(i64 %a) {
  %x = zext i8 bitcast (<8 x i1> <i1 true, i1 false, i1 true, i1 false, i1 false, i1 true, i1 false, i1 true> to i8) to i64
  %y = zext i16 bitcast (<2 x i8> <i8 1, i8 2> to i16) to i64
  %w = zext i8 bitcast (<4 x i2> <i2 1, i2 -2, i2 -1, i2 0> to i8) to i64
  %z = add i64 %a, bitcast (<2 x i32> <i32 7, i32 -1> to i64)
  %f = bitcast float bitcast (<2 x half> <half 1.0, half -2.0> to float) to i32
  %fz = zext i32 %f to i64
  %d = bitcast double bitcast (<4 x i16> <i16 1, i16 2, i16 3, i16 16384> to double) to i64
  %s1 = shl i64 %y, 8
  %s2 = shl i64 %w, 24
  %o1 = or i64 %x, %s1
  %o2 = or i64 %o1, %s2
  %o3 = xor i64 %o2, %z
  %o4 = xor i64 %o3, %fz
  %o5 = xor i64 %o4, %d
  ret i64 %o5
}
";

#[test]
fn vector_bitcasts_to_scalars_agree_with_llc() {
    let calls: Vec<native::Call> =
        [0, 1, u64::MAX, 1 << 40].map(|a| ("vbits".to_owned(), [a, 0, 0, 0, 0, 0])).to_vec();
    run("a64-tail2-vector-bits", VECTOR_BITS, &calls, None);
}

// ---- a block address taken only by an unreachable indirectbr ----

const UNREACHABLE_INDIRECTBR: &str = "
@ua = global ptr blockaddress(@ub, %t), align 8

define i64 @ub(i64 %a) {
entry:
  %p = load ptr, ptr @ua, align 8
  %n = icmp ne ptr %p, null
  %z = zext i1 %n to i64
  %r = add i64 %z, %a
  ret i64 %r
dead:
  %x = load ptr, ptr @ua, align 8
  indirectbr ptr %x, [label %t]
t:
  ret i64 7
}
";

#[test]
fn a_block_address_of_an_unreachable_indirectbr_is_placed() {
    let calls: Vec<native::Call> =
        [0, 5, u64::MAX].map(|a| ("ub".to_owned(), [a, 0, 0, 0, 0, 0])).to_vec();
    run("a64-tail2-unreachable-indirectbr", UNREACHABLE_INDIRECTBR, &calls, None);
}

// ---- preserve_mostcc ----

const PRESERVE_MOST: &str = "
define internal preserve_mostcc i64 @pm_mix(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g, i64 %h, i64 %i) {
  %x = mul i64 %a, %i
  %y = sub i64 %b, %h
  %z = xor i64 %x, %y
  %w = add i64 %z, %c
  %v = udiv i64 %w, 3
  %u = or i64 %v, %g
  ret i64 %u
}

define i64 @pm_call(i64 %a, i64 %b) {
  %r = call preserve_mostcc i64 @pm_mix(i64 %a, i64 %b, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, i64 9)
  %s = call preserve_mostcc i64 @pm_mix(i64 %r, i64 %a, i64 %b, i64 1, i64 2, i64 3, i64 4, i64 5, i64 %r)
  ret i64 %s
}
";

/// As caller and callee, with a stack argument.
#[test]
fn preserve_most_calls_agree_with_the_interpreter() {
    let mut rng = Rng(0x0952_0095);
    let calls = calls_of("pm_call", &mut rng, 30);
    let want = interp_lines(PRESERVE_MOST, &calls);
    run("a64-tail2-preserve-most", PRESERVE_MOST, &calls, Some(&want));
}

/// A caller in assembly puts sentinels in `x9`..`x15` and calls a
/// `preserve_mostcc` function that calls C. The C callee unwinds the
/// stack: in the caller's frame the unwinder must find the sentinels
/// (only the callee's rules say where they are), and after the return the
/// registers must hold them.
const PM_UNWIND_IR: &str = "
declare void @probe()

define preserve_mostcc i64 @pm(i64 %a) {
  call void @probe()
  %r = add i64 %a, 1
  ret i64 %r
}
";

const PM_UNWIND_C: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <unwind.h>

uint64_t call_pm_entry(void) __asm__("call_pm");

__asm__(
    ".text\n.globl call_pm\n.type call_pm, %function\ncall_pm:\n"
    ".cfi_startproc\n"
    "stp x29, x30, [sp, #-16]!\n.cfi_def_cfa_offset 16\n"
    ".cfi_offset 29, -16\n.cfi_offset 30, -8\nmov x29, sp\n"
    "mov x9, #0x909\nmov x10, #0xa0a\nmov x11, #0xb0b\nmov x12, #0xc0c\n"
    "mov x13, #0xd0d\nmov x14, #0xe0e\nmov x15, #0xf0f\nmov x0, #41\n"
    "bl pm\n"
    "mov x1, #0x909\ncmp x9, x1\nb.ne 1f\nmov x1, #0xa0a\ncmp x10, x1\nb.ne 1f\n"
    "mov x1, #0xb0b\ncmp x11, x1\nb.ne 1f\nmov x1, #0xc0c\ncmp x12, x1\nb.ne 1f\n"
    "mov x1, #0xd0d\ncmp x13, x1\nb.ne 1f\nmov x1, #0xe0e\ncmp x14, x1\nb.ne 1f\n"
    "mov x1, #0xf0f\ncmp x15, x1\nb.ne 1f\n"
    "ldp x29, x30, [sp], #16\nret\n"
    "1:\nmov x0, #999\nldp x29, x30, [sp], #16\nret\n"
    ".cfi_endproc\n"
);

static uint64_t seen[7];
static int found;

static _Unwind_Reason_Code step(struct _Unwind_Context *ctx, void *arg) {
    (void)arg;
    uintptr_t ip = _Unwind_GetIP(ctx);
    /* The return address into the caller, which is 31 instructions. */
    uintptr_t start = (uintptr_t)&call_pm_entry;
    if (ip > start && ip < start + 31 * 4) {
        for (int r = 0; r < 7; r++) seen[r] = _Unwind_GetGR(ctx, 9 + r);
        found = 1;
    }
    return _URC_NO_REASON;
}

__attribute__((noinline)) void probe(void) {
    _Unwind_Backtrace(step, 0);
    /* Clobber what the convention lets a callee clobber. */
    __asm__ volatile("mov x9, #1\nmov x10, #1\nmov x11, #1\nmov x12, #1\n"
                     "mov x13, #1\nmov x14, #1\nmov x15, #1"
                     ::: "x9", "x10", "x11", "x12", "x13", "x14", "x15");
}

int main(void) {
    uint64_t r = call_pm_entry();
    printf("%llu %d", (unsigned long long)r, found);
    for (int i = 0; i < 7; i++) printf(" %llx", (unsigned long long)seen[i]);
    printf("\n");
    return 0;
}
"#;

#[test]
fn preserve_most_saves_and_describes_x9_to_x15() {
    let dir = hod_testutil::Scratch::new("a64-tail2-pm-unwind");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", PM_UNWIND_C, &[]);
    let (module, bodies) = parse(&format!("{}{PM_UNWIND_IR}", ll_header(22)));
    let mut objects = Vec::new();
    for (name, options) in &MODES {
        let emitted = compile_module(&module, &bodies, options).expect("compiles");
        objects.push((*name, write_object(&dir, &format!("{name}.o"), &emitted)));
    }
    if let Some(object) =
        llc(&dir, "llc", &format!("{}{PM_UNWIND_IR}", ll_header(llc_major())), &[])
    {
        objects.push(("llc", object));
    }
    let want = "42 1 909 a0a b0b c0c d0d e0e f0f\n";
    for linker in &toolchain.linkers {
        for (name, object) in &objects {
            let exe = dir.join(format!("{name}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, object], &exe);
            assert!(ran.success(), "{name} under {}: {}", linker.name, ran.stderr_text());
            assert_eq!(ran.stdout_text(), want, "{name} under {}", linker.name);
        }
    }
}

// ---- llvm.global.annotations and fmv-features ----

const ANNOTATED: &str = r#"
@.str = private unnamed_addr constant [5 x i8] c"note\00", section "llvm.metadata"
@.file = private unnamed_addr constant [4 x i8] c"a.c\00", section "llvm.metadata"
@llvm.global.annotations = appending global [1 x { ptr, ptr, ptr, i32, ptr }] [{ ptr, ptr, ptr, i32, ptr } { ptr @f, ptr @.str, ptr @.file, i32 3, ptr null }], section "llvm.metadata"

define i64 @f(i64 %a) #0 {
  %r = add i64 %a, 1
  ret i64 %r
}

attributes #0 = { "fmv-features"="dotprod,sve2" }
"#;

/// The annotations global in `llvm.metadata` is dropped (`llc` writes
/// nothing of it), and `"fmv-features"` is dropped: the module classifies
/// clean and runs.
#[test]
fn annotations_and_fmv_features_are_dropped() {
    let (module, bodies) = parse(&format!("{}{ANNOTATED}", ll_header(22)));
    let notes: Vec<String> = module.unsupported().map(|n| n.construct.to_string()).collect();
    assert!(notes.is_empty(), "{notes:?}");
    hod_ir::classify(&module).expect("the module classifies");
    for body in &bodies {
        hod_ir::classify_body(&module, body).expect("the body classifies");
    }
    let calls = vec![("f".to_owned(), [41, 0, 0, 0, 0, 0])];
    run("a64-tail2-annotations", ANNOTATED, &calls, Some("42\n"));
}

/// Outside `llvm.metadata` the annotations global stays refused.
#[test]
fn annotations_outside_llvm_metadata_are_noted() {
    let text = ANNOTATED
        .replace("ptr null }], section \"llvm.metadata\"", "ptr null }], section \"other\"");
    let (module, _) = parse(&format!("{}{text}", ll_header(22)));
    let notes: Vec<String> = module.unsupported().map(|n| n.construct.to_string()).collect();
    assert!(notes.iter().any(|n| n == "special-global:llvm.global.annotations"), "{notes:?}");
}

/// Compiling must not fail on what was refused before: every function of
/// each table above compiles on its own (a guard for the tables).
#[test]
fn tables_compile() {
    for text in [EXTRACT_I128, VECTOR_BITS, PRESERVE_MOST] {
        compile_ll(text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    }
    compile_ll(&bfloat_body()).expect("bfloat");
}
