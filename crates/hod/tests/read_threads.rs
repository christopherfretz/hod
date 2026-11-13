//! `spec/cli.md` "Threads": how many threads read the bodies (a pure
//! function of the body text, the body count and the threads granted),
//! and one request to the token pool per compile, shared by reading and
//! code generation, every token given back.

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hod::{
    Options, OutputKind, READ_BYTES_PER_THREAD, READ_MAX_THREADS, READ_MIN_BODY_BYTES, Report,
    Settings, ThreadTokens, Threads, read_threads,
};

#[test]
fn the_reading_policy() {
    let large = 100 * READ_BYTES_PER_THREAD;
    // Nothing to share out.
    assert_eq!(read_threads(0, 0, 8), 1);
    assert_eq!(read_threads(large, 1, 8), 1);
    // One thread granted, one thread.
    assert_eq!(read_threads(large, 1000, 1), 1);
    assert_eq!(read_threads(large, 1000, 0), 1);
    // At most what is granted, and never more than READ_MAX_THREADS.
    assert_eq!(read_threads(large, 1000, 2), 2);
    assert_eq!(read_threads(large, 1000, 3), 3);
    assert_eq!(read_threads(large, 1000, 8), READ_MAX_THREADS);
    assert_eq!(read_threads(large, 1000, 64), READ_MAX_THREADS);
    // Each thread needs READ_BYTES_PER_THREAD: below twice that, one.
    let bodies = 2 * READ_BYTES_PER_THREAD / READ_MIN_BODY_BYTES;
    assert_eq!(read_threads(2 * READ_BYTES_PER_THREAD - 1, bodies, 8), 1);
    assert_eq!(read_threads(2 * READ_BYTES_PER_THREAD, bodies, 8), 2);
    assert_eq!(read_threads(3 * READ_BYTES_PER_THREAD, bodies, 8), 3);
    // Small bodies stay on one thread, however many.
    assert_eq!(read_threads(large, large / (READ_MIN_BODY_BYTES - 1), 8), 1);
    assert_eq!(read_threads(large, large / READ_MIN_BODY_BYTES, 8), READ_MAX_THREADS);
    // The measured inputs (spec/landed/ll/WP-063.md): tiny-functions
    // (5,001 bodies of 132 bytes on average) stays sequential; the
    // regex unit (1,711 bodies, 2.4 MB) takes four.
    assert_eq!(read_threads(661_181, 5_001, 8), 1);
    assert_eq!(read_threads(2_477_206, 1_711, 8), 4);
    assert_eq!(read_threads(2_477_206, 1_711, 2), 2);
}

/// Counts requests and holds the idle tokens.
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

/// A module of `count` functions of about 60 instructions and 2 KB of
/// text each; function `refused` asks for a feature hod refuses.
fn module(count: usize, refused: Option<usize>) -> String {
    let mut out = String::from("target triple = \"aarch64-unknown-linux-gnu\"\n");
    out.push_str("@g = global i64 0, align 8\n");
    for index in 0..count {
        let features = if Some(index) == refused {
            "+strict-align"
        } else {
            "+fp-armv8,+neon,+outline-atomics,+v8a,-fmv"
        };
        let _ = write!(
            out,
            "define i64 @f{index}(i64 %a, i64 %b) #{index} {{\nentry:\n  %s = alloca i64, align 8\n  \
             store i64 %a, ptr %s, align 8\n  br label %loop\nloop:\n  \
             %i = phi i64 [ 0, %entry ], [ %next, %body ]\n  %done = icmp sge i64 %i, %b\n  \
             br i1 %done, label %exit, label %body\nbody:\n  %v = load i64, ptr %s, align 8\n"
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
            "  %next = add i64 %i, 1\n  br label %loop\nexit:\n  %r = load i64, ptr %s, align 8\n  \
             ret i64 %r\n}}\nattributes #{index} = {{ nounwind \"target-features\"=\"{features}\" }}\n"
        );
    }
    out
}

fn compile(input: &str, threads: Threads) -> Result<Vec<u8>, hod::CompileError> {
    let settings = Settings { threads, ..Settings::default() };
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(input.as_bytes(), &options, &settings, &mut Report::default())
}

#[test]
fn reading_and_code_generation_share_one_request() {
    let large = module(1200, None);
    assert!(large.len() > 4 * READ_BYTES_PER_THREAD, "large enough to read in parallel");
    let alone = compile(&large, Threads { max: 1, ..Threads::default() });
    assert!(alone.is_ok());
    for idle in [0, 1, 2, 3, 7, 15] {
        let pool = Arc::new(Pool { idle: AtomicUsize::new(idle), asked: AtomicUsize::new(0) });
        let threads = Threads { max: 8, tokens: Some(pool.clone()), per_thread: 0 };
        assert_eq!(compile(&large, threads), alone, "{idle} idle");
        assert_eq!(pool.asked.load(Ordering::SeqCst), 1, "{idle} idle: one request");
        assert_eq!(pool.idle.load(Ordering::SeqCst), idle, "{idle} idle: every token back");
    }
    // Refused in code generation: the held tokens come back too.
    let refused = module(1200, Some(700));
    let pool = Arc::new(Pool { idle: AtomicUsize::new(5), asked: AtomicUsize::new(0) });
    let threads = Threads { max: 8, tokens: Some(pool.clone()), per_thread: 0 };
    assert!(compile(&refused, threads).is_err());
    assert_eq!(pool.asked.load(Ordering::SeqCst), 1);
    assert_eq!(pool.idle.load(Ordering::SeqCst), 5);
    // Small: no request at all.
    let small = module(3, None);
    let pool = Arc::new(Pool { idle: AtomicUsize::new(5), asked: AtomicUsize::new(0) });
    let threads = Threads { max: 8, tokens: Some(pool.clone()), per_thread: 0 };
    assert!(compile(&small, threads).is_ok());
    assert_eq!(pool.asked.load(Ordering::SeqCst), 0);
    assert_eq!(pool.idle.load(Ordering::SeqCst), 5);
}
