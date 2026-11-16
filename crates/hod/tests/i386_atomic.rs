//! WP-316 (design 0008 I11b): atomics on i386 (`spec/codegen-x86.md`
//! §12). `atomic/ops.c` (every `atomicrmw` operation of clang's
//! builtins, `cmpxchg` strong and weak with several ordering pairs,
//! loads, stores and fences at 8, 16, 32 and 64 bits; under PIE a PLT
//! call follows every `cmpxchg8b` sequence, which took `ebx`) and
//! `atomic/threads.c` (four threads incrementing counters of each width,
//! a `cmpxchg` loop, a `cmpxchg` spin lock guarding a plain update: a
//! missing `lock` loses updates) at `-march=i586` (no `cmov`), the
//! default CPU and `-march=pentium4`, static and PIE, by `llc` 19 and 22
//! and hod; the runs under `qemu-i386` must print the same totals.

use hod_testutil::Scratch;

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, data, lane, refusal};

/// The CPUs: clang's flags and the QEMU features their code may use.
const CPUS: [(&str, &[&str], &[&str]); 3] = [
    ("i586", &["-march=i586"], &[]),
    ("i686", &[], &[]),
    ("p4", &["-march=pentium4"], &["mmx", "sse", "sse2"]),
];

fn run_all(program: &str, link: &[&str], all_linkers: bool) {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new(&format!("i386-atomic-{program}"));
    let source = data("atomic", program);
    let mut runs = 0;
    for (cpu, flags, features) in CPUS {
        for pic in [false, true] {
            let build = Build { pic, features, link, all_linkers, ..STATIC };
            let name = format!("{program}-{cpu}");
            runs += lane.check(&scratch, &name, &Source::C(&source, flags), build);
        }
    }
    eprintln!("i386 atomic {program}: {runs} runs equal llc's");
}

#[test]
fn atomic_operations_run_as_llc_s() {
    run_all("ops.c", &[], true);
}

#[test]
fn threads_lose_no_update() {
    run_all("threads.c", &["-pthread"], false);
}

/// The x87 barrier shapes (`spec/codegen-x86.md` §11, §12 "Barriers"):
/// each puts one construct of WP-316 between an x87 result and its use,
/// `(name, construct)`. The construct may use `%m` (an 8-byte cell
/// holding 3 on entry) and store a result through `%o2`. A `va` name is
/// a variadic shape (called with `i32 11, double 2.5`).
const BARRIER_SHAPES: [(&str, &str); 28] = [
    ("xchg32", "%r = atomicrmw xchg ptr %m, i32 5 seq_cst\n  store i32 %r, ptr %o2"),
    ("add8", "%r = atomicrmw add ptr %m, i8 4 seq_cst\n  store i8 %r, ptr %o2"),
    ("sub16_unused", "%r = atomicrmw sub ptr %m, i16 2 seq_cst"),
    ("and32", "%r = atomicrmw and ptr %m, i32 6 seq_cst\n  store i32 %r, ptr %o2"),
    ("and32_unused", "%r = atomicrmw and ptr %m, i32 6 seq_cst"),
    ("or8_unused", "%r = atomicrmw or ptr %m, i8 64 monotonic"),
    ("xor16", "%r = atomicrmw xor ptr %m, i16 255 acq_rel\n  store i16 %r, ptr %o2"),
    ("nand32", "%r = atomicrmw nand ptr %m, i32 6 seq_cst\n  store i32 %r, ptr %o2"),
    ("max32_unused", "%r = atomicrmw max ptr %m, i32 9 seq_cst"),
    ("umin8", "%r = atomicrmw umin ptr %m, i8 2 seq_cst\n  store i8 %r, ptr %o2"),
    ("add64", "%r = atomicrmw add ptr %m, i64 4294967297 seq_cst\n  store i64 %r, ptr %o2"),
    ("xchg64", "%r = atomicrmw xchg ptr %m, i64 -1 seq_cst\n  store i64 %r, ptr %o2"),
    ("and64_unused", "%r = atomicrmw and ptr %m, i64 2 seq_cst"),
    (
        "cmpxchg32",
        "%r = cmpxchg ptr %m, i32 3, i32 9 seq_cst seq_cst\n  \
         %v = extractvalue { i32, i1 } %r, 0\n  store i32 %v, ptr %o2",
    ),
    (
        "cmpxchg64",
        "%r = cmpxchg ptr %m, i64 3, i64 -9 seq_cst seq_cst\n  \
         %v = extractvalue { i64, i1 } %r, 0\n  store i64 %v, ptr %o2",
    ),
    (
        "cmpxchg64_weak",
        "%r = cmpxchg weak ptr %m, i64 4, i64 9 acq_rel monotonic\n  \
         %f = extractvalue { i64, i1 } %r, 1\n  store i1 %f, ptr %o2",
    ),
    ("load64", "%r = load atomic i64, ptr %m seq_cst, align 8\n  store i64 %r, ptr %o2"),
    ("store64", "store atomic i64 -77, ptr %m seq_cst, align 8"),
    ("load8", "%r = load atomic i8, ptr %m acquire, align 1\n  store i8 %r, ptr %o2"),
    ("store32", "store atomic i32 21, ptr %m seq_cst, align 4"),
    ("fence", "fence seq_cst"),
    ("fence_acquire", "fence acquire"),
    (
        "alloca",
        "%sz = load volatile i32, ptr @size\n  %d = alloca i8, i32 %sz, align 4\n  \
         store volatile i8 7, ptr %d\n  %dv = load volatile i8, ptr %d\n  store i8 %dv, ptr %o2",
    ),
    (
        "alloca64",
        "%sz = load volatile i32, ptr @size\n  %d = alloca i8, i32 %sz, align 64\n  \
         %da = ptrtoint ptr %d to i32\n  %dm = and i32 %da, 63\n  store i32 %dm, ptr %o2",
    ),
    (
        "stacksave",
        "%s = call ptr @llvm.stacksave.p0()\n  %sz = load volatile i32, ptr @size\n  \
         %d = alloca i8, i32 %sz, align 4\n  store volatile i8 7, ptr %d\n  \
         call void @llvm.stackrestore.p0(ptr %s)",
    ),
    (
        "va_copy",
        "%ap = alloca ptr\n  %aq = alloca ptr\n  call void @llvm.va_start.p0(ptr %ap)\n  \
         call void @llvm.va_copy.p0(ptr %aq, ptr %ap)\n  call void @llvm.va_end.p0(ptr %aq)\n  \
         call void @llvm.va_end.p0(ptr %ap)",
    ),
    (
        "va_arg_i32",
        "%ap = alloca ptr\n  call void @llvm.va_start.p0(ptr %ap)\n  \
         %v = va_arg ptr %ap, i32\n  store i32 %v, ptr %o2\n  call void @llvm.va_end.p0(ptr %ap)",
    ),
    (
        "va_arg_double",
        "%ap = alloca ptr\n  call void @llvm.va_start.p0(ptr %ap)\n  \
         %v0 = va_arg ptr %ap, i32\n  %v = va_arg ptr %ap, double\n  store double %v, ptr %o2\n  \
         call void @llvm.va_end.p0(ptr %ap)",
    ),
];

/// The barrier module of `shapes` for `ty` (`float` or `double`), its
/// functions carrying `"target-cpu"=cpu` (none: `generic`): one shape
/// per function computes `p = a * b` (80 bits: `1 + 2^-k` squared),
/// stores `p - n`, runs its construct, then stores `p + c`, which is
/// `2^-2k` while `p` keeps 80 bits and 0 once it is rounded (the driver
/// of `x87/shapes.c`, in IR). `main` prints one line per shape: the bits
/// of the three result slots and the cell.
fn barrier_module(shapes: &[(&str, &str)], ty: &str, cpu: Option<&str>) -> String {
    use std::fmt::Write as _;
    let (bits, a, c, nc) = if ty == "float" {
        ("i32", "1065355264", "-1082126336", "1065357312")
    } else {
        ("i64", "4607182418804211712", "-4616189618046369792", "4607182418808406016")
    };
    let mut t = format!(
        "target triple = \"i686-unknown-linux-gnu\"\n\
         @in = global [4 x {bits}] [{bits} {a}, {bits} {a}, {bits} {c}, {bits} {nc}]\n\
         @out = global [3 x i64] zeroinitializer, align 8\n\
         @m = global i64 3, align 8\n@size = global i32 24\n\
         @fmt = private constant [26 x i8] c\"%2d: %llx %llx %llx %llx\\0A\\00\"\n\
         declare i32 @printf(ptr, ...)\ndeclare ptr @llvm.stacksave.p0()\n\
         declare void @llvm.stackrestore.p0(ptr)\ndeclare void @llvm.va_start.p0(ptr)\n\
         declare void @llvm.va_copy.p0(ptr, ptr)\ndeclare void @llvm.va_end.p0(ptr)\n"
    );
    let mut main = String::from("define i32 @main() #0 {\nentry:\n");
    for (k, (name, construct)) in shapes.iter().enumerate() {
        let va = name.starts_with("va");
        writeln!(
            t,
            "define void @{name}(ptr %in, ptr %out, ptr %m{}) #0 {{\nentry:\n  \
             %a = load {ty}, ptr %in\n  %gb = getelementptr {ty}, ptr %in, i32 1\n  \
             %b = load {ty}, ptr %gb\n  %gc = getelementptr {ty}, ptr %in, i32 2\n  \
             %c = load {ty}, ptr %gc\n  %gn = getelementptr {ty}, ptr %in, i32 3\n  \
             %n = load {ty}, ptr %gn\n  %o1 = getelementptr i8, ptr %out, i32 8\n  \
             %o2 = getelementptr i8, ptr %out, i32 16\n  %p = fmul {ty} %a, %b\n  \
             %x = fsub {ty} %p, %n\n  store {ty} %x, ptr %out\n  {construct}\n  \
             %y = fadd {ty} %p, %c\n  store {ty} %y, ptr %o1\n  ret void\n}}",
            if va { ", ..." } else { "" }
        )
        .unwrap();
        let extra = if va { ", i32 11, double 2.5" } else { "" };
        let sig = if va { " (ptr, ptr, ptr, ...)" } else { "" };
        writeln!(
            main,
            "  store i64 3, ptr @m\n  store i64 0, ptr @out\n  \
             store i64 0, ptr getelementptr (i8, ptr @out, i32 8)\n  \
             store i64 0, ptr getelementptr (i8, ptr @out, i32 16)\n  \
             call void{sig} @{name}(ptr @in, ptr @out, ptr @m{extra})\n  \
             %r0.{k} = load i64, ptr @out\n  \
             %r1.{k} = load i64, ptr getelementptr (i8, ptr @out, i32 8)\n  \
             %r2.{k} = load i64, ptr getelementptr (i8, ptr @out, i32 16)\n  \
             %m.{k} = load i64, ptr @m\n  \
             call i32 (ptr, ...) @printf(ptr @fmt, i32 {k}, i64 %r0.{k}, i64 %r1.{k}, \
             i64 %r2.{k}, i64 %m.{k})"
        )
        .unwrap();
    }
    main.push_str("  ret i32 0\n}\n");
    t.push_str(&main);
    match cpu {
        Some(cpu) => writeln!(t, "attributes #0 = {{ nounwind \"target-cpu\"=\"{cpu}\" }}"),
        None => writeln!(t, "attributes #0 = {{ nounwind }}"),
    }
    .unwrap();
    t
}

/// Every barrier shape runs as `llc`'s, in `float` and `double`, at CPU
/// `generic` and `pentiumpro` (`cmov`), hod plain (`hod_x86` directly:
/// the facade refuses the IR `va_arg`) and under `Stress::ALL`.
#[test]
fn barrier_shapes_run_as_llc_s() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-atomic-barriers");
    let mut lines = 0;
    for ty in ["float", "double"] {
        for cpu in [None, Some("pentiumpro")] {
            let text = barrier_module(&BARRIER_SHAPES, ty, cpu);
            let name = format!("barriers-{ty}-{}", cpu.unwrap_or("generic"));
            let build = Build { back_end: true, ..STATIC };
            let runs = lane.check(&scratch, &name, &Source::Ll(&text), build);
            // Three runs (llc, hod, stress) per LLVM version.
            lines += runs / 3 * BARRIER_SHAPES.len();
        }
    }
    eprintln!("i386 x87 barrier shapes: {lines} shape lines equal llc's");
}

/// The shapes that stay refused, each alone in its module.
#[test]
fn what_stays_refused() {
    let cases = [
        // `llc` calls `__atomic_load_8` on a CPU without `cmpxchg8b`.
        (
            "define i64 @f(ptr %p) {\n  %v = load atomic i64, ptr %p seq_cst, align 8\n  ret i64 %v\n}",
            Some("i486"),
            "x86:atomic:no-cx8",
        ),
        // `__atomic_load` (libatomic's generic routine).
        (
            "define void @f(ptr %p, ptr %q) {\n  %v = load atomic i128, ptr %p seq_cst, align 16\n  store i128 %v, ptr %q\n  ret void\n}",
            None,
            "x86:atomic:libatomic",
        ),
        (
            "define i32 @f(ptr %p) {\n  %v = load atomic i32, ptr %p seq_cst, align 2\n  ret i32 %v\n}",
            None,
            "x86:atomic:libatomic",
        ),
        (
            "define float @f(ptr %p, float %v) {\n  %o = atomicrmw fmax ptr %p, float %v seq_cst\n  ret float %o\n}",
            None,
            "x86:atomic:float-op",
        ),
        (
            "define i32 @f(ptr %p, i32 %v) {\n  %o = atomicrmw uinc_wrap ptr %p, i32 %v seq_cst\n  ret i32 %o\n}",
            None,
            "x86:atomic:op",
        ),
    ];
    for (body, cpu, name) in cases {
        let error = refusal(body, cpu).unwrap_or_else(|| panic!("compiled:\n{body}"));
        assert!(error.contains(name), "{body}\n{error}");
    }
    // What compiles alone: a `float` atomic load, store and `xchg`, a
    // single-thread fence, a 64-bit store on `pentium4`, `fadd` and
    // `fsub` (lowered since join D; run in `i386_join_d.rs`).
    let fine = [
        "define double @f(ptr %p, ptr %q, float %v, double %w) {\n  \
         %o = atomicrmw fadd ptr %p, float %v seq_cst\n  \
         %d = atomicrmw fsub ptr %q, double %w seq_cst\n  ret double %d\n}",
        "define float @f(ptr %p, float %v) {\n  store atomic float %v, ptr %p seq_cst, align 4\n  \
         %o = atomicrmw xchg ptr %p, float %v monotonic\n  %w = load atomic float, ptr %p acquire, align 4\n  \
         fence syncscope(\"singlethread\") seq_cst\n  ret float %w\n}",
        "define void @f(ptr %p, i64 %v) {\n  store atomic i64 %v, ptr %p seq_cst, align 8\n  ret void\n}",
    ];
    for body in fine {
        assert_eq!(refusal(body, Some("pentium4")), None, "{body}");
        assert_eq!(refusal(body, None), None, "{body}");
    }
}
