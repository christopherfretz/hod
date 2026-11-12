//! `spec/ll.md` §8: whatever the bytes, the lexer and the parser return.
//! They do not panic, do not recurse without bound and do not take time
//! out of proportion to the input: every prefix of the hand-written
//! modules, seeded mutations of them, a sample of prefixes and mutations
//! of what clang writes, and inputs built to be deep, long or repetitive.

mod common;

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use hod_ir::Arch;
use hod_ll::{Lexer, Tok};

use common::{Lcg, Llvm, handwritten, parse};

/// Lexes and parses `text`; whether it parsed. An error must lie inside
/// the text.
fn survive(text: &[u8]) -> bool {
    let mut lexer = Lexer::new(text);
    let mut tokens = 0usize;
    loop {
        let token = lexer.next_token();
        if matches!(token.kind, Tok::Eof | Tok::Error) {
            break;
        }
        assert!(token.start < token.end, "a token is not empty");
        tokens += 1;
        assert!(tokens <= text.len(), "more tokens than bytes");
    }
    match parse(text, Arch::X86_64) {
        Ok(_) => true,
        Err(error) => {
            assert!(error.offset <= text.len(), "{error}: offset past the end");
            false
        }
    }
}

/// Runs `job` on each item, a thread each, eight at a time: the machine
/// is shared.
fn each<T: Sync, R: Send>(items: &[T], job: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let mut results = Vec::new();
    for batch in items.chunks(8) {
        std::thread::scope(|scope| {
            let workers: Vec<_> = batch.iter().map(|item| scope.spawn(|| job(item))).collect();
            results.extend(workers.into_iter().map(|worker| worker.join().expect("a worker")));
        });
    }
    results
}

/// One mutation of `text`: bytes replaced, a range removed, a range
/// repeated elsewhere, or a piece of syntax dropped in.
fn mutate(text: &[u8], rng: &mut Lcg) -> Vec<u8> {
    const PIECES: [&[u8]; 24] = [
        b"{", b"}", b"(", b")", b"[", b"]", b"<", b">", b"\"", b"\\", b";", b"/*", b"*/", b"!",
        b"@", b"%", b"$", b"#", b"^", b",", b"=", b"\n", b"\0", b"define ",
    ];
    let mut out = text.to_vec();
    let at = rng.below(text.len());
    match rng.below(4) {
        0 => {
            for _ in 0..=rng.below(3) {
                let at = rng.below(out.len());
                out[at] = rng.below(256) as u8;
            }
        }
        1 => {
            let len = rng.below(64).min(out.len() - at);
            out.drain(at..at + len);
        }
        2 => {
            let from = rng.below(text.len());
            let len = rng.below(200).min(text.len() - from);
            out.splice(at..at, text[from..from + len].iter().copied());
        }
        _ => {
            let piece = PIECES[rng.below(PIECES.len())];
            out.splice(at..at, piece.iter().copied());
        }
    }
    out
}

/// §8: every prefix of every hand-written module, a byte at a time.
#[test]
fn every_prefix_of_the_handwritten_modules_returns() {
    let files = handwritten();
    let counts = each(&files, |(name, text)| {
        assert!(survive(text), "{name} parses");
        let parsed = (0..text.len()).filter(|&len| survive(&text[..len])).count();
        (name.clone(), text.len(), parsed)
    });
    for (name, len, parsed) in counts {
        println!("{name}: {len} prefixes, {parsed} of them modules");
        // A prefix that ends between two entities is a module.
        assert!(parsed > 10 && parsed < len, "{name}: some prefixes parse and some do not");
    }
}

/// §8: seeded mutations of the hand-written modules.
#[test]
fn mutations_of_the_handwritten_modules_return() {
    const ROUNDS: usize = 1500;
    let files = handwritten();
    let counts = each(&files, |(name, text)| {
        let mut rng = Lcg(0x5eed ^ text.len() as u64);
        let parsed = (0..ROUNDS).filter(|_| survive(&mutate(text, &mut rng))).count();
        (name.clone(), parsed)
    });
    for (name, parsed) in counts {
        println!("{name}: {ROUNDS} mutations, {parsed} of them modules");
        assert!(parsed < ROUNDS, "{name}: some mutation is an error");
    }
}

/// §8: a sample of prefixes and of mutations of a module that clang
/// wrote with debug information, which has the metadata syntax the
/// hand-written modules have little of.
#[test]
fn prefixes_and_mutations_of_clang_output_return() {
    const SAMPLES: usize = 96;
    let cases: Vec<(Llvm, Arch)> = Llvm::all()
        .into_iter()
        .flat_map(|llvm| common::ARCHES.map(|arch| (llvm.clone(), arch)))
        .collect();
    let counts = each(&cases, |(llvm, arch)| {
        let text = llvm.clang("globals.c", *arch, &["-g"]);
        assert!(survive(&text), "clang {} output parses", llvm.major);
        let mut rng = Lcg(u64::from(llvm.major) << 8 | *arch as u64);
        let mut parsed = 0;
        for _ in 0..SAMPLES {
            parsed += usize::from(survive(&text[..rng.below(text.len())]));
            parsed += usize::from(survive(&mutate(&text, &mut rng)));
        }
        (llvm.major, text.len(), parsed)
    });
    for (major, len, parsed) in counts {
        println!("clang {major}: {len} bytes, {parsed} of {} variants are modules", 2 * SAMPLES);
    }
}

/// Inputs built to be deep, long or repetitive, by name.
fn pathological() -> Vec<(&'static str, Vec<u8>)> {
    const DEEP: usize = 100_000;
    const LONG: usize = 1 << 20;
    let repeat = |prefix: &str, unit: &str, count: usize| {
        let mut text = prefix.as_bytes().to_vec();
        for _ in 0..count {
            text.extend_from_slice(unit.as_bytes());
        }
        text
    };
    let lines = |count: usize, line: &dyn Fn(usize) -> String| {
        let mut text = String::new();
        for index in 0..count {
            text.push_str(&line(index));
            text.push('\n');
        }
        text.into_bytes()
    };
    let mut inputs = vec![
        ("parentheses in a value", repeat("@x = global i32 ", "(", DEEP)),
        ("brackets in a value", repeat("@x = global [1 x i8] ", "[", DEEP)),
        ("braces in a value", repeat("@x = global { i8 } ", "{ ", DEEP)),
        ("angles in a value", repeat("@x = global <1 x i8> ", "<", DEEP)),
        ("array types", repeat("@x = global ", "[1 x ", DEEP)),
        ("struct types", repeat("%T = type ", "{ ", DEEP)),
        ("vector types", repeat("%T = type ", "<2 x ", DEEP)),
        ("packed struct types", repeat("%T = type ", "<{ ", DEEP)),
        ("function types", repeat("%T = type void ", "(void ", DEEP)),
        ("typed pointers", repeat("@x = global i8", "*", LONG)),
        ("metadata tuples", repeat("!0 = ", "!{", DEEP)),
        ("debug nodes", repeat("!0 = ", "!DIExpression(", DEEP)),
        ("constant expressions", repeat("@x = global ptr ", "getelementptr (i8, ptr ", DEEP)),
        ("casts", repeat("@x = global i64 ", "ptrtoint (ptr inttoptr (i64 ", DEEP)),
        ("parameters", repeat("declare void @f", "(", DEEP)),
        ("parameter attributes", repeat("declare void @f(ptr ", "byval(", DEEP)),
        ("attribute arguments", repeat("attributes #0 = { memory", "(", DEEP)),
        ("bodies", repeat("define void @f() ", "{", DEEP)),
        ("closers", repeat("define void @f() {", "}", DEEP)),
        ("mixed brackets", repeat("@x = global i32 ", "([{<", DEEP)),
        ("a string that does not end", repeat("@x = global [1 x i8] c\"", "a", LONG)),
        ("escapes", repeat("@x = global [1 x i8] c\"", "\\", LONG)),
        ("quotes in a body", repeat("define void @f() {", "\"", LONG)),
        ("a comment that does not end", repeat("/*", "*", LONG)),
        ("a line comment", repeat(";", ";", LONG)),
        ("a long name", repeat("@", "a", LONG)),
        ("a long word", repeat("", "a", LONG)),
        ("a long number", repeat("@x = global i32 ", "7", LONG)),
        ("a long hexadecimal number", repeat("@x = global double 0x", "F", LONG)),
        ("minus signs", repeat("@x = global i32 ", "-", LONG)),
        ("dots", repeat("@x = global double 1", ".", LONG)),
        ("sigils", repeat("", "@%$!#^", LONG / 6)),
        ("zero bytes", vec![0; LONG]),
        ("high bytes", vec![0xff; LONG]),
        ("commas", repeat("@x = global i32 0", ", ", LONG / 2)),
        ("alignments", repeat("@x = global i32 0", ", align 4", DEEP)),
        ("attributes", repeat("declare void @f()", " nounwind", DEEP)),
        ("string attributes", repeat("attributes #0 = {", " \"k\"=\"v\"", DEEP)),
        ("one group many times", repeat("", "attributes #0 = { nounwind }\n", DEEP)),
        ("module asm", repeat("", "module asm \"nop\"\n", DEEP)),
    ];
    let chain = DEEP / 2;
    inputs.extend([
        ("types that name the next", lines(chain, &|n| format!("%T{n} = type {{ %T{} }}", n + 1))),
        (
            "types that hold the next",
            lines(chain, &|n| match chain - 1 - n {
                0 => format!("%T{n} = type {{ i8 }}"),
                _ => format!("%T{n} = type {{ ptr, %T{} }}", n + 1),
            }),
        ),
        ("types in a ring", lines(chain, &|n| format!("%T{n} = type {{ %T{} }}", (n + 1) % chain))),
        (
            "types that hold the last",
            lines(chain, &|n| match n {
                0 => "%T0 = type { i8 }".to_owned(),
                _ => format!("%T{n} = type {{ ptr, %T{} }}", n - 1),
            }),
        ),
        (
            "numbered types, descending",
            lines(chain, &|n| format!("%{} = type {{ i8 }}", chain - n)),
        ),
        ("numbered globals", lines(chain, &|n| format!("@{n} = private global ptr @{}", n / 2))),
        (
            "globals that name the next",
            lines(chain, &|n| format!("@g{n} = global ptr @g{}", (n + 1) % chain)),
        ),
        (
            "aliases of the next",
            lines(chain, &|n| {
                if n + 1 == chain {
                    format!("@a{n} = global i8 0")
                } else {
                    format!("@a{n} = alias i8, ptr @a{}", n + 1)
                }
            }),
        ),
        (
            "aliases in a ring",
            lines(chain, &|n| format!("@a{n} = alias i8, ptr @a{}", (n + 1) % chain)),
        ),
        (
            "aliases of the last",
            lines(chain, &|n| match n {
                0 => "@a0 = global i8 0".to_owned(),
                _ => format!("@a{n} = alias i8, ptr @a{}", n - 1),
            }),
        ),
        ("metadata, descending", lines(chain, &|n| format!("!{} = !{{!{}}}", chain - n, n))),
        ("attribute groups", lines(chain, &|n| format!("attributes #{} = {{ cold }}", chain - n))),
        ("comdats", lines(chain, &|n| format!("$c{n} = comdat any"))),
        ("declarations", lines(chain, &|n| format!("declare void @f{n}(ptr noundef) #{n}"))),
        ("use-list orders", lines(chain, &|_| "uselistorder ptr @x, { 1, 0 }".to_owned())),
    ]);
    // Block addresses that go back and forth between two large functions.
    let mut blocks = String::new();
    for function in ["f", "g"] {
        let _ = writeln!(blocks, "define void @{function}() {{");
        for label in 0..2000 {
            let _ = writeln!(blocks, "b{label}:\n  br label %b{}", label + 1);
        }
        blocks.push_str("b2000:\n  ret void\n}\n");
    }
    for index in 0..20_000 {
        let function = if index % 2 == 0 { "f" } else { "g" };
        let label = index % 2001;
        let _ = writeln!(blocks, "@t{index} = global ptr blockaddress(@{function}, %b{label})");
    }
    inputs.push(("block addresses in two functions", blocks.into_bytes()));
    inputs
}

/// The pathological inputs that are modules; the others are errors.
const MODULES: [&str; 19] = [
    "a line comment",
    "zero bytes",
    "alignments",
    "attributes",
    "one group many times",
    "module asm",
    "types that hold the next",
    "types that hold the last",
    "numbered types, descending",
    "numbered globals",
    "globals that name the next",
    "aliases of the next",
    "aliases of the last",
    "metadata, descending",
    "attribute groups",
    "comdats",
    "declarations",
    "use-list orders",
    "block addresses in two functions",
];

/// §8: deep, long and repetitive inputs return, without a stack
/// overflow, in time that a hang or a quadratic loop would far exceed.
#[test]
fn pathological_inputs_return_in_time() {
    const LIMIT: Duration = Duration::from_secs(30);
    let inputs = pathological();
    let times = each(&inputs, |(name, text)| {
        let start = Instant::now();
        let parsed = survive(text);
        (*name, text.len(), parsed, start.elapsed())
    });
    for (name, len, parsed, time) in times {
        let verdict = if parsed { "a module" } else { "an error" };
        println!("{name}: {len} bytes, {verdict}, {} ms", time.as_millis());
        assert!(time < LIMIT, "{name}: {time:?} for {len} bytes");
        let module = MODULES.contains(&name);
        assert_eq!(parsed, module, "{name}: {verdict}");
    }
}
