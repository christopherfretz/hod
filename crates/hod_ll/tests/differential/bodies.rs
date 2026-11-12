//! `spec/ll.md` §11 against LLVM: the bodies of the corpus, as written
//! and as `llvm-dis` prints them.

use std::collections::BTreeSet;

use hod_ir::{Body, Module};
use hod_ll::{BodyParser, ParsedModule};

use crate::common::{corpus, parse, parsed};

/// What a body of clang's output may be unsupported for (`spec/ll.md`
/// §3.4); any other name fails the test.
const CLANG_BODY_UNSUPPORTED: &[&str] = &[
    "attr:convergent",
    "attr:no_caller_saved_registers",
    "attr:nocf_check",
    "attr:noduplicate",
    "call:forwarded-varargs",
    "inst:catchpad",
    "inst:catchret",
    "inst:catchswitch",
    "inst:cleanuppad",
    "inst:cleanupret",
];

/// A function's name, and its body's blocks and instructions.
type Shape = (Vec<u8>, Option<(usize, usize)>);

/// Every body of `parsed`: by function name, the body (or `None`, with
/// the reasons), each verified.
fn bodies(name: &str, parsed: &mut ParsedModule<'_>) -> Vec<(Vec<u8>, Option<Body>, Vec<String>)> {
    let ParsedModule { module, text } = parsed;
    let mut parser = BodyParser::new(text);
    let mut out = Vec::new();
    for function in text.functions() {
        let body = parser.parse(module, function).unwrap_or_else(|error| panic!("{name}: {error}"));
        if let Some(body) = &body {
            body.verify(module).unwrap_or_else(|error| panic!("{name}: verify: {error}"));
        }
        let reasons = parser.unsupported().map(str::to_owned).collect();
        out.push((module.symbol(function.sym).name().to_vec(), body, reasons));
    }
    out
}

/// The blocks of each body of a canonical text, in order, with the
/// function's name: the
/// entry, and one for each label line (`llvm-dis` writes every other
/// block's label, at the start of a line, with nothing else before it).
fn canonical_blocks(text: &[u8]) -> Vec<(Vec<u8>, usize)> {
    let mut out = Vec::new();
    let mut open: Option<(Vec<u8>, usize, bool)> = None;
    for line in text.split(|&byte| byte == b'\n') {
        if let Some((name, blocks, first)) = &mut open {
            if line == b"}" {
                out.push((std::mem::take(name), *blocks));
                open = None;
                continue;
            }
            let label = !line.is_empty() && !matches!(line[0], b' ' | b';');
            if label && !*first {
                *blocks += 1;
            }
            if !line.is_empty() && line[0] != b';' {
                *first = false;
            }
            continue;
        }
        if line.starts_with(b"define ") && line.ends_with(b"{") {
            let at = line.iter().position(|&byte| byte == b'@').expect("a function name");
            let rest = &line[at + 1..];
            let name = if rest.first() == Some(&b'"') {
                let end = rest[1..].iter().position(|&byte| byte == b'"').expect("a quote");
                rest[1..=end].to_vec()
            } else {
                let end = rest.iter().position(|&byte| byte == b'(').expect("parameters");
                rest[..end].to_vec()
            };
            open = Some((name, 1, true));
        }
    }
    out
}

/// §11: every body of the corpus parses and verifies, as written and as
/// `llvm-dis` prints it; the two readings have the same blocks and
/// instructions; each has as many blocks as `llvm-dis` prints labels for;
/// and what is unsupported is known by name.
#[test]
fn every_body_parses_verifies_and_has_llvms_blocks() {
    let (mut checked, mut unsupported) = (0usize, BTreeSet::new());
    for case in corpus() {
        let written = bodies(&case.name, &mut parsed(case));
        let mut canonical_module = parse(&case.canonical, case.arch).expect("accepted");
        let canonical = bodies(&case.name, &mut canonical_module);
        let mut shape = |list: &[(Vec<u8>, Option<Body>, Vec<String>)]| {
            let mut shapes: Vec<Shape> = list
                .iter()
                .map(|(name, body, reasons)| {
                    unsupported.extend(reasons.iter().cloned());
                    (name.clone(), body.as_ref().map(|b| (b.block_count(), b.inst_count())))
                })
                .collect();
            shapes.sort();
            shapes
        };
        let written_shapes = shape(&written);
        let canonical_shapes = shape(&canonical);
        assert_eq!(written_shapes, canonical_shapes, "{}: the two readings differ", case.name);
        // The functions of the canonical text, in its order, which is
        // that of `text.functions()`.
        let theirs = canonical_blocks(&case.canonical);
        assert_eq!(theirs.len(), canonical.len(), "{}: definitions", case.name);
        for ((name, blocks), (_, body, _)) in theirs.iter().zip(&canonical) {
            if let Some(body) = body {
                let name = String::from_utf8_lossy(name);
                assert_eq!(body.block_count(), *blocks, "{}: blocks of @{name}", case.name);
                checked += 1;
            }
        }
    }
    let unknown: Vec<&String> = unsupported
        .iter()
        .filter(|name| !CLANG_BODY_UNSUPPORTED.contains(&name.as_str()))
        .collect();
    assert!(unknown.is_empty(), "unsupported constructs not known by name: {unknown:?}");
    println!("{checked} bodies checked; unsupported: {unsupported:?}");
}

/// §11.1: bodies parsed in parallel, each worker with a clone of the
/// module, are the bodies one sequential parser gives against that
/// module.
#[test]
fn bodies_parsed_on_clones_in_parallel_are_the_sequential_ones() {
    const WORKERS: usize = 3;
    let Some(case) = corpus().iter().max_by_key(|case| case.text.len()) else {
        return;
    };
    let mut parsed = parsed(case);
    // A first pass adds what the bodies intern; the second gives the
    // bodies to compare against, and adds nothing more.
    bodies(&case.name, &mut parsed);
    let before = (parsed.module.types.len(), parsed.module.consts.len());
    let serial = bodies(&case.name, &mut parsed);
    assert_eq!(before, (parsed.module.types.len(), parsed.module.consts.len()));
    let ParsedModule { module, text } = &parsed;
    let functions = text.functions();
    let parallel: Vec<(usize, Option<Body>)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..WORKERS)
            .map(|worker| {
                let mut clone: Module = module.clone();
                scope.spawn(move || {
                    let mut parser = BodyParser::new(text);
                    let mut out = Vec::new();
                    for at in (worker..functions.len()).step_by(WORKERS) {
                        let body = parser.parse(&mut clone, &functions[at]).expect("parses");
                        out.push((at, body));
                    }
                    out
                })
            })
            .collect();
        workers.into_iter().flat_map(|worker| worker.join().expect("a worker")).collect()
    });
    assert_eq!(parallel.len(), serial.len());
    for (at, body) in parallel {
        assert!(body == serial[at].1, "{}: body {at} differs", case.name);
    }
}
