//! The back half of `hod::compile` (`spec/cli.md` "The back half",
//! "Threads"): the object does not depend on the thread count, a refusal
//! in any chunk refuses the module with the first in module order, and
//! tokens taken for threads are given back.

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hod::{
    CompileError, Construct, Options, OutputKind, PARALLEL_MIN_INSTS, Report, Settings,
    ThreadTokens, Threads, Unsupported,
};

const TRIPLE: &str = "target triple = \"aarch64-unknown-linux-gnu\"\n";

fn obj() -> Options {
    Options { output_kind: OutputKind::Object, ..Options::default() }
}

/// A function of integer arithmetic, a loop and a global, about 60
/// instructions; `features` is its `target-features` string.
fn function(out: &mut String, index: usize, features: &str) {
    let _ = write!(
        out,
        "define i64 @f{index}(i64 %a, i64 %b) #{index} {{
entry:
  %s = alloca i64, align 8
  store i64 %a, ptr %s, align 8
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %next, %body ]
  %done = icmp sge i64 %i, %b
  br i1 %done, label %exit, label %body
body:
  %v = load i64, ptr %s, align 8
"
    );
    for step in 0..12 {
        let _ = write!(
            out,
            "  %m{step} = mul i64 %v, {k}\n  %x{step} = xor i64 %m{step}, %i\n  \
             %y{step} = add i64 %x{step}, {index}\n  store i64 %y{step}, ptr %s, align 8\n",
            k = step + 3
        );
    }
    let _ = write!(
        out,
        "  %g = load i64, ptr @g, align 8
  %gs = add i64 %g, %i
  store i64 %gs, ptr @g, align 8
  %next = add i64 %i, 1
  br label %loop
exit:
  %r = load i64, ptr %s, align 8
  ret i64 %r
}}
attributes #{index} = {{ nounwind \"target-features\"=\"{features}\" }}
"
    );
}

/// A module of `count` functions; `features(i)` is function `i`'s
/// feature string.
fn module(count: usize, features: impl Fn(usize) -> &'static str) -> String {
    let mut out = format!("{TRIPLE}@g = global i64 0, align 8\n");
    for index in 0..count {
        function(&mut out, index, features(index));
    }
    out
}

const PLAIN: &str = "+fp-armv8,+neon,+outline-atomics,+v8a,-fmv";

fn compile(input: &str, threads: Threads) -> Result<Vec<u8>, CompileError> {
    let settings = Settings { threads, ..Settings::default() };
    hod::compile_with(input.as_bytes(), &obj(), &settings, &mut Report::default())
}

fn max(max: usize) -> Threads {
    Threads { max, ..Threads::default() }
}

/// Instructions per function of [`function`], about.
const PER_FUNCTION: usize = 60;

// spec/cli.md "Threads", docs/ARCHITECTURE.md principle 5: the same
// object for every thread count, and for repeated runs.
#[test]
fn object_is_the_same_for_every_thread_count() {
    let count = 9 * PARALLEL_MIN_INSTS / PER_FUNCTION;
    let input = module(count, |_| PLAIN);
    let one = compile(&input, max(1)).expect("hod compiles the module");
    assert!(one.starts_with(b"\x7FELF"));
    for threads in [2, 3, 8, 1, 8] {
        let object = compile(&input, max(threads)).expect("hod compiles the module");
        assert!(object == one, "{threads} threads give another object");
    }
    // A small module, which stays on one thread whatever it may use.
    let small = module(3, |_| PLAIN);
    assert_eq!(compile(&small, max(8)), compile(&small, max(1)));
}

/// Hands out at most `idle` tokens and counts what is held.
#[derive(Debug)]
struct Pool {
    idle: AtomicUsize,
    asked: AtomicUsize,
}

impl ThreadTokens for Pool {
    fn take(&self, wanted: usize) -> usize {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let idle = self.idle.load(Ordering::SeqCst);
        let got = idle.min(wanted);
        self.idle.store(idle - got, Ordering::SeqCst);
        got
    }

    fn give_back(&self, count: usize) {
        self.idle.fetch_add(count, Ordering::SeqCst);
    }
}

// spec/cli.md "Threads": tokens are asked for only above the break-even,
// at most one fewer than the threads wanted, and all come back, also
// when the module is refused.
#[test]
fn tokens_are_asked_for_large_modules_and_given_back() {
    let pool = Arc::new(Pool { idle: AtomicUsize::new(2), asked: AtomicUsize::new(0) });
    let threads = || Threads { max: 8, tokens: Some(pool.clone()), per_thread: 0 };
    let small = module(3, |_| PLAIN);
    compile(&small, threads()).expect("hod compiles the module");
    assert_eq!(pool.asked.load(Ordering::SeqCst), 0, "a small module asks for nothing");
    let large = module(9 * PARALLEL_MIN_INSTS / PER_FUNCTION, |_| PLAIN);
    let alone = compile(&large, max(1));
    assert_eq!(compile(&large, threads()), alone);
    assert_eq!(pool.asked.load(Ordering::SeqCst), 1);
    assert_eq!(pool.idle.load(Ordering::SeqCst), 2, "the tokens are back");
    let refused = module(9 * PARALLEL_MIN_INSTS / PER_FUNCTION, |i| {
        if i == 300 { "+strict-align" } else { PLAIN }
    });
    assert!(compile(&refused, threads()).is_err());
    assert_eq!(pool.idle.load(Ordering::SeqCst), 2, "the tokens are back after a refusal");
}

fn feature_refusal(reason: &str, function: usize, feature: &str) -> Result<Vec<u8>, CompileError> {
    Err(Unsupported {
        construct: Construct::Named(reason.to_owned().into()),
        function: Some(format!("f{function}")),
        detail: Some(feature.to_owned()),
    }
    .into())
}

// spec/cli.md "The back half": a refusal in any chunk refuses the module,
// and the one reported is the first in module order, whichever thread
// met it first.
#[test]
fn the_first_refusal_in_module_order_wins() {
    let count = 9 * PARALLEL_MIN_INSTS / PER_FUNCTION;
    for (early, late) in [(5, count - 3), (count / 2, count - 1), (count - 2, count - 1)] {
        let input = module(count, |i| {
            if i == early {
                "+neon,+strict-align"
            } else if i == late {
                "+bogus"
            } else {
                PLAIN
            }
        });
        let expected = feature_refusal("attr:target-feature-strict-align", early, "+strict-align");
        for threads in [1, 2, 3, 8] {
            assert_eq!(compile(&input, max(threads)), expected, "{threads} threads, f{early}");
        }
    }
    // Only a late one: it is the answer.
    let input = module(count, |i| if i == count - 1 { "+bogus" } else { PLAIN });
    let expected = feature_refusal("attr:target-feature-unknown", count - 1, "+bogus");
    for threads in [1, 8] {
        assert_eq!(compile(&input, max(threads)), expected, "{threads} threads");
    }
}

// spec/cli.md "The back half": target features, of a function and of
// `-mattr`; what changes nothing is accepted.
#[test]
fn target_features() {
    let accepted = module(2, |i| {
        if i == 0 {
            "+aes,+crc,+fp-armv8,+lse,+neon,+outline-atomics,+v8.1a,+v8a,-fmv"
        } else {
            "+bti,+pauth,-sve"
        }
    });
    assert!(compile(&accepted, max(1)).is_ok());
    for (features, reason) in [
        ("-fp-armv8", "attr:target-feature-soft-float"),
        ("-neon", "attr:target-feature-no-neon"),
        ("+reserve-x18", "attr:target-feature-reserve"),
        ("+tagged-globals", "attr:target-feature-tagged-globals"),
        ("+frobnicate", "attr:target-feature-unknown"),
    ] {
        let input = module(2, |i| if i == 1 { features } else { PLAIN });
        assert_eq!(compile(&input, max(1)), feature_refusal(reason, 1, features));
    }
    let plain = module(2, |_| PLAIN);
    let mattr = Options { features: vec!["+neon".into(), "+strict-align".into()], ..obj() };
    let refused =
        hod::compile_with(plain.as_bytes(), &mattr, &Settings::default(), &mut Report::default());
    assert_eq!(
        refused,
        Err(Unsupported {
            construct: Construct::Named("attr:target-feature-strict-align".into()),
            function: None,
            detail: Some("+strict-align".into()),
        }
        .into())
    );
    let fine = Options { features: vec!["+lse".into(), "-crc".into()], ..obj() };
    assert!(
        hod::compile_with(plain.as_bytes(), &fine, &Settings::default(), &mut Report::default())
            .is_ok()
    );
}

// spec/cli.md "The back half": x86-64 has no code generator yet.
#[test]
fn x86_64_is_a_stable_refusal() {
    let input = "target triple = \"x86_64-unknown-linux-gnu\"\n\
                 define i32 @f() nounwind {\n  ret i32 1\n}\n";
    let support = hod_ir::Support::everything_implemented(hod_ir::Arch::X86_64);
    let settings = Settings { support: Some(support), ..Settings::default() };
    let result = hod::compile_with(input.as_bytes(), &obj(), &settings, &mut Report::default());
    assert_eq!(result, Err(Unsupported::module(Construct::CodegenNotImplemented).into()));
}

// Measures the break-even of `PARALLEL_MIN_INSTS` (spec/landed/cli/WP-043.md):
// `cargo test --release -p hod --test back -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a test"]
fn break_even() {
    use std::time::Instant;
    for functions in [8, 16, 32, 64, 128, 256, 512, 1024, 4096] {
        let input = module(functions, |_| PLAIN);
        let mut line = format!("{:>7} insts:", functions * PER_FUNCTION);
        for threads in [1, 2, 4, 8] {
            let settings = Settings {
                threads: Threads { max: threads, per_thread: 1, ..Threads::default() },
                ..Settings::default()
            };
            let mut best = u128::MAX;
            for _ in 0..15 {
                let mut report = Report::default();
                hod::compile_with(input.as_bytes(), &obj(), &settings, &mut report).unwrap();
                let at = |name| report.phases.iter().find(|(n, _)| *n == name).unwrap().1;
                best = best.min((at("emission") - at("bodies")).as_micros());
            }
            let _ = write!(line, "  {threads}t {best:>6} us");
        }
        println!("{line}");
    }
}
