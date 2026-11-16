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
            "define float @f(ptr %p, float %v) {\n  %o = atomicrmw fadd ptr %p, float %v seq_cst\n  ret float %o\n}",
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
    // single-thread fence, a 64-bit store on `pentium4`.
    let fine = [
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
