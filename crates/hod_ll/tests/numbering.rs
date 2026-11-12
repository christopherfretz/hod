//! `spec/ll.md` §11.2, §11.3: the numbers of a body's unnamed values and
//! blocks, at the scale of a large function.
//!
//! - The reduced case of WP-071: a block the label scan met more than
//!   4096 numbers ahead (so in the sparse table), used backwards after a
//!   later number grew the dense table over it.
//! - Generated functions (fixed seeds) of thousands of numbered blocks
//!   and values, referred to forwards and backwards by `br`, `switch`,
//!   `indirectbr`, `invoke`, `phi` and `blockaddress`, with numbers
//!   skipped (a few, and more than 4096 at once), blocks of more than
//!   4096 instructions and blocks without a label. Each module: `llvm-as`
//!   19 and 22 accept it; every block's successors in `hod_ll`'s body are
//!   the ones in `llvm-as | llvm-dis`'s text; and the module as hod's
//!   printer writes it has, through `llvm-as | llvm-dis`, the same block
//!   references (branch targets, `phi` incomings, `blockaddress`es) as
//!   the original.
//! - Refusals: numbers that go back (a value, a block, an argument, one
//!   number twice) and an undefined number, refused by `hod_ll` at
//!   `llvm-as`'s position with `llvm-as`'s words.
//!
//! `HOD_LL_NUMBERING_DUMP=dir` writes each generated module and hod's
//! printing of it there.

mod common;

use std::fmt::Write as _;

use hod_ir::{Body, Dialect, Module, TargetSpec};
use hod_ll::{BodyParser, ParseOptions, parse_module};

use common::Llvm;

fn options() -> ParseOptions {
    ParseOptions { default_target: Some(TargetSpec::aarch64_linux_gnu()) }
}

/// Parses `text`, module and bodies: the module and each function's
/// body, in order. Panics on an error or an unsupported construct.
fn read(text: &[u8]) -> (Module, Vec<Body>) {
    let mut parsed = parse_module(text, &options()).expect("the module parses");
    let mut parser = BodyParser::new(&parsed.text);
    let mut bodies = Vec::new();
    for function in parsed.text.functions() {
        let body = parser.parse(&mut parsed.module, function).unwrap_or_else(|error| {
            panic!("{error}");
        });
        let reasons: Vec<&str> = parser.unsupported().collect();
        let body = body.unwrap_or_else(|| panic!("unsupported: {reasons:?}"));
        body.verify(&parsed.module).expect("a valid body");
        bodies.push(body);
    }
    (parsed.module, bodies)
}

/// The first error `hod_ll` gives `text`, as `line:column: message`.
fn refusal(text: &[u8]) -> Option<String> {
    let mut parsed = match parse_module(text, &options()) {
        Ok(parsed) => parsed,
        Err(error) => return Some(format!("{}:{}: {}", error.line, error.column, error.message)),
    };
    let mut parser = BodyParser::new(&parsed.text);
    for function in parsed.text.functions() {
        if let Err(error) = parser.parse(&mut parsed.module, function) {
            return Some(format!("{}:{}: {}", error.line, error.column, error.message));
        }
    }
    None
}

// ---- the reduced case ----

/// The shape of the `huge-function` bench input that was refused: a
/// forward branch early (the label scan puts every label in the tables),
/// a block of more than 4096 values (the labels after it go to the
/// sparse table), then a loop whose header is used backwards after its
/// own values grew the dense table over its number.
fn reduced() -> String {
    let mut text = String::from("define void @f(ptr %p) {\n  br label %1\n1:\n");
    for number in 2..4102 {
        let _ = writeln!(text, "  %{number} = load volatile i8, ptr %p, align 1");
    }
    text.push_str(
        "  br label %4102\n\
         4102:\n  %4103 = load volatile i1, ptr %p, align 1\n  \
         br i1 %4103, label %4102, label %4104\n\
         4104:\n  ret void\n}\n",
    );
    text
}

#[test]
fn a_block_beyond_the_dense_table_is_found_after_the_table_grows_over_it() {
    let text = reduced();
    for llvm in Llvm::all() {
        llvm.accepts(text.as_bytes())
            .unwrap_or_else(|error| panic!("llvm-as {}: {error}", llvm.major));
    }
    let (_, bodies) = read(text.as_bytes());
    let body = &bodies[0];
    assert_eq!(body.block_count(), 4);
    let successors: Vec<Vec<usize>> = body
        .blocks()
        .map(|block| block.successors().map(hod_ir::BlockId::index).collect())
        .collect();
    assert_eq!(successors, [vec![1], vec![2], vec![2, 3], vec![]]);
}

// ---- generated functions ----

/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// In `0..n`.
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).expect("small")
    }

    /// True with probability `percent` in 100.
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

/// How one generated function is shaped.
#[derive(Clone, Copy)]
struct Shape {
    blocks: usize,
    /// Most instructions in an ordinary block.
    insts: usize,
    /// Percent of blocks with more than 4096 instructions.
    huge: usize,
    /// Percent of blocks after the entry written without a label.
    unlabelled: usize,
    /// Percent of written numbers that skip some.
    skips: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Term {
    Br,
    CondBr,
    Switch,
    IndirectBr,
    Invoke,
    Ret,
    /// A landing pad block's `resume`.
    Resume,
}

/// One block: its number, whether its label is written, its terminator
/// and targets (by block index), and the numbers of its values.
struct Block {
    label: Option<u32>,
    number: u32,
    term: Term,
    targets: Vec<usize>,
    /// The landing pad an `invoke` unwinds to.
    unwind: usize,
    phi: Option<u32>,
    /// `(number, written)`: an instruction written without a name takes
    /// the next number.
    insts: Vec<(u32, bool)>,
    /// The `icmp` of a `condbr`, the result of an `invoke`, the value of
    /// a landing pad.
    extra: Option<u32>,
    /// Blocks whose address the block stores.
    addresses: Vec<usize>,
    preds: Vec<usize>,
}

/// The next number: `next`, or more if written and `skip` says so.
fn number(rng: &mut Rng, next: &mut u32, written: bool, skips: usize) -> u32 {
    let mut at = *next;
    if written && rng.chance(skips) {
        at += if rng.chance(10) { 4097 + rng.below(3000) as u32 } else { 1 + rng.below(3) as u32 };
    }
    *next = at + 1;
    at
}

/// The blocks of a function: block 0 is the entry, block 1 and the last
/// block are landing pads (unwound to backwards and forwards), the others
/// a chain where each block's first target is the next one and the other
/// targets are anywhere (never the entry, never a landing pad).
fn plan(rng: &mut Rng, shape: Shape) -> Vec<Block> {
    let n = shape.blocks.max(4);
    let normal: Vec<usize> = (2..n - 1).collect();
    let mut blocks: Vec<Block> = Vec::with_capacity(n);
    for index in 0..n {
        let pad = index == 1 || index == n - 1;
        let term = if pad {
            Term::Resume
        } else if index == n - 2 {
            Term::Ret
        } else {
            match rng.below(10) {
                0..=2 => Term::Br,
                3..=5 => Term::CondBr,
                6 => Term::Switch,
                7 => Term::IndirectBr,
                _ => Term::Invoke,
            }
        };
        let mut targets = Vec::new();
        if term != Term::Ret && term != Term::Resume {
            targets.push(if index == 0 { 2 } else { index + 1 });
            let extra = match term {
                Term::CondBr => 1,
                Term::Switch | Term::IndirectBr => 1 + rng.below(4),
                _ => 0,
            };
            while targets.len() < 1 + extra {
                let target = normal[rng.below(normal.len())];
                if !targets.contains(&target) {
                    targets.push(target);
                }
            }
        }
        let unwind = if rng.chance(50) { 1 } else { n - 1 };
        let mut addresses = Vec::new();
        if !pad && rng.chance(5) {
            addresses.push(normal[rng.below(normal.len())]);
        }
        blocks.push(Block {
            label: None,
            number: 0,
            term,
            targets,
            unwind,
            phi: None,
            insts: Vec::new(),
            extra: None,
            addresses,
            preds: Vec::new(),
        });
    }
    for index in 0..n {
        for at in 0..blocks[index].targets.len() {
            let target = blocks[index].targets[at];
            blocks[target].preds.push(index);
        }
    }
    // The numbers: the arguments are %0 and %1.
    let mut next = 2u32;
    for (index, block) in blocks.iter_mut().enumerate() {
        let written = index != 0 && !rng.chance(shape.unlabelled);
        block.number = number(rng, &mut next, written, shape.skips);
        block.label = written.then_some(block.number);
        let pad = block.term == Term::Resume;
        if pad {
            block.extra = Some(number(rng, &mut next, true, shape.skips));
            continue;
        }
        if !block.preds.is_empty() && rng.chance(40) {
            block.phi = Some(number(rng, &mut next, true, shape.skips));
        }
        let count = if rng.chance(shape.huge) {
            4100 + rng.below(1000)
        } else {
            1 + rng.below(shape.insts)
        };
        for _ in 0..count {
            let written = !rng.chance(10);
            block.insts.push((number(rng, &mut next, written, shape.skips), written));
        }
        if matches!(block.term, Term::CondBr | Term::Invoke) {
            block.extra = Some(number(rng, &mut next, true, shape.skips));
        }
    }
    blocks
}

/// Writes function `name` of `shape`.
fn function(rng: &mut Rng, name: &str, shape: Shape, out: &mut String) {
    let blocks = plan(rng, shape);
    let block_ref = |index: usize| format!("%{}", blocks[index].number);
    let _ = writeln!(out, "define i32 @{name}(i32 %0, ptr %1) personality ptr @personality {{");
    for block in &blocks {
        if let Some(label) = block.label {
            let _ = writeln!(out, "{label}:");
        }
        if block.term == Term::Resume {
            let value = block.extra.expect("a landing pad's value");
            let _ = writeln!(out, "  %{value} = landingpad {{ ptr, i32 }} cleanup");
            let _ = writeln!(out, "  resume {{ ptr, i32 }} %{value}");
            continue;
        }
        let mut last = "%0".to_string();
        if let Some(phi) = block.phi {
            let incoming: Vec<String> = block
                .preds
                .iter()
                .map(|&pred| {
                    let value = blocks[pred].insts.last().expect("an instruction").0;
                    format!("[ %{value}, {} ]", block_ref(pred))
                })
                .collect();
            let _ = writeln!(out, "  %{phi} = phi i32 {}", incoming.join(", "));
            last = format!("%{phi}");
        }
        for &(value, written) in &block.insts {
            let op = ["add", "xor", "mul", "sub"][rng.below(4)];
            let rhs =
                if rng.chance(20) { "%0".to_string() } else { (rng.below(100) + 1).to_string() };
            if written {
                let _ = writeln!(out, "  %{value} = {op} i32 {last}, {rhs}");
            } else {
                let _ = writeln!(out, "  {op} i32 {last}, {rhs}");
            }
            last = format!("%{value}");
        }
        for &address in &block.addresses {
            let _ = writeln!(
                out,
                "  store volatile ptr blockaddress(@{name}, {}), ptr %1, align 8",
                block_ref(address)
            );
        }
        let t = &block.targets;
        match block.term {
            Term::Br => {
                let _ = writeln!(out, "  br label {}", block_ref(t[0]));
            }
            Term::CondBr => {
                let cond = block.extra.expect("an icmp");
                let _ = writeln!(out, "  %{cond} = icmp ult i32 {last}, 50");
                let _ = writeln!(
                    out,
                    "  br i1 %{cond}, label {}, label {}",
                    block_ref(t[0]),
                    block_ref(t[1])
                );
            }
            Term::Switch => {
                let _ = writeln!(out, "  switch i32 {last}, label {} [", block_ref(t[0]));
                for (case, &target) in t[1..].iter().enumerate() {
                    let _ = writeln!(out, "    i32 {case}, label {}", block_ref(target));
                }
                out.push_str("  ]\n");
            }
            Term::IndirectBr => {
                let to = t[rng.below(t.len())];
                let labels: Vec<String> =
                    t.iter().map(|&target| format!("label {}", block_ref(target))).collect();
                let _ = writeln!(
                    out,
                    "  indirectbr ptr blockaddress(@{name}, {}), [{}]",
                    block_ref(to),
                    labels.join(", ")
                );
            }
            Term::Invoke => {
                let result = block.extra.expect("an invoke's result");
                let _ = writeln!(
                    out,
                    "  %{result} = invoke i32 @callee(i32 {last})\n          to label {} unwind label {}",
                    block_ref(t[0]),
                    block_ref(block.unwind)
                );
            }
            Term::Ret => {
                let _ = writeln!(out, "  ret i32 {last}");
            }
            Term::Resume => unreachable!("written above"),
        }
    }
    out.push_str("}\n\n");
}

/// A module of `shapes.len()` generated functions.
fn module(seed: u64, shapes: &[Shape]) -> String {
    let mut rng = Rng(seed);
    let mut out = String::from(
        "target triple = \"aarch64-unknown-linux-gnu\"\n\n\
         declare i32 @personality(...)\n\
         declare i32 @callee(i32)\n\n",
    );
    for (index, &shape) in shapes.iter().enumerate() {
        function(&mut rng, &format!("f{index}"), shape, &mut out);
    }
    out
}

/// Per function of an `llvm-dis` text, per block: the blocks it refers
/// to (by ordinal), in order, each tagged by how: `label %b` (`L`), a
/// `phi`'s incoming block (`P`), a `blockaddress` (`A`).
fn references(text: &str) -> Vec<Vec<Vec<(char, usize)>>> {
    let mut functions = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if !line.starts_with("define ") {
            continue;
        }
        let mut body: Vec<&str> = Vec::new();
        for line in lines.by_ref() {
            if line == "}" {
                break;
            }
            body.push(line);
        }
        // The blocks: a label line starts a block unless the current
        // one has nothing yet.
        let mut names: Vec<Option<String>> = vec![None];
        let mut contents: Vec<Vec<&str>> = vec![Vec::new()];
        for line in body {
            let code = line.split(';').next().unwrap_or("").trim_end();
            if code.is_empty() {
                continue;
            }
            if !line.starts_with(' ') && code.ends_with(':') {
                let name = code.trim_end_matches(':').trim_matches('"').to_string();
                if contents.last().is_some_and(|block| !block.is_empty()) {
                    names.push(None);
                    contents.push(Vec::new());
                }
                *names.last_mut().expect("a block") = Some(name);
                continue;
            }
            contents.last_mut().expect("a block").push(code);
        }
        // `llvm-dis` writes no label for an entry block without a name:
        // its number is the one after the unnamed arguments'.
        if names[0].is_none() {
            let unnamed = line.split(['(', ' ', ',', ')']).filter(|word| {
                word.strip_prefix('%').is_some_and(|rest| rest.bytes().all(|b| b.is_ascii_digit()))
            });
            names[0] = Some(unnamed.count().to_string());
        }
        let ordinal = |name: &str| {
            names
                .iter()
                .position(|known| known.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("no block {name}"))
        };
        let mut blocks = Vec::new();
        for lines in &contents {
            let mut refs = Vec::new();
            for line in lines {
                let words: Vec<&str> = line
                    .split(|c: char| {
                        c == ' ' || c == ',' || c == '[' || c == ']' || c == '(' || c == ')'
                    })
                    .filter(|word| !word.is_empty())
                    .collect();
                let phi = words.contains(&"phi");
                for at in 0..words.len() {
                    let word = words[at];
                    let Some(name) = word.strip_prefix('%') else { continue };
                    let name = name.trim_matches('"');
                    if at > 0 && words[at - 1] == "label" {
                        refs.push(('L', ordinal(name)));
                    } else if at > 1 && words[at - 2] == "blockaddress" {
                        refs.push(('A', ordinal(name)));
                    } else if phi && at > 0 && names.contains(&Some(name.to_string())) {
                        // `[ value, %block ]`: values and blocks have
                        // distinct names.
                        refs.push(('P', ordinal(name)));
                    }
                }
            }
            blocks.push(refs);
        }
        functions.push(blocks);
    }
    functions
}

/// Checks the module of `seed` (see the file's comment).
fn check(seed: u64, shapes: &[Shape], llvm: &[Llvm]) {
    let text = module(seed, shapes);
    for llvm in llvm {
        llvm.accepts(text.as_bytes())
            .unwrap_or_else(|error| panic!("seed {seed}: llvm-as {} refuses: {error}", llvm.major));
    }
    let (module, bodies) = read(text.as_bytes());
    let Some(newest) = llvm.last() else { return };
    let canonical = newest.canonical(text.as_bytes()).expect("llvm-as | llvm-dis");
    let canonical = String::from_utf8(canonical).expect("text");
    let want = references(&canonical);
    assert_eq!(want.len(), bodies.len(), "seed {seed}: functions");
    for (function, (body, blocks)) in bodies.iter().zip(&want).enumerate() {
        assert_eq!(body.block_count(), blocks.len(), "seed {seed}: f{function}: blocks");
        for (ordinal, (block, refs)) in body.blocks().zip(blocks).enumerate() {
            let mine: Vec<usize> = block.successors().map(hod_ir::BlockId::index).collect();
            let theirs: Vec<usize> =
                refs.iter().filter(|(how, _)| *how == 'L').map(|&(_, target)| target).collect();
            assert_eq!(mine, theirs, "seed {seed}: f{function}: successors of block {ordinal}");
        }
    }
    // The printer's text, read back by LLVM, refers to the same blocks.
    let mut printed = String::new();
    module
        .print(&mut printed, Dialect::new(newest.major).expect("a dialect"), &bodies)
        .expect("printed");
    if let Ok(dir) = std::env::var("HOD_LL_NUMBERING_DUMP") {
        std::fs::write(format!("{dir}/{seed}.ll"), &text).expect("dump");
        std::fs::write(format!("{dir}/{seed}.hod.ll"), &printed).expect("dump");
    }
    let again = newest
        .canonical(printed.as_bytes())
        .unwrap_or_else(|error| panic!("seed {seed}: printed text: {error}"));
    let again = references(&String::from_utf8(again).expect("text"));
    for (function, (mine, theirs)) in again.iter().zip(&want).enumerate() {
        for (ordinal, (mine, theirs)) in mine.iter().zip(theirs).enumerate() {
            assert_eq!(mine, theirs, "seed {seed}: printed f{function}: block {ordinal}");
        }
        assert_eq!(mine.len(), theirs.len(), "seed {seed}: printed f{function}: blocks");
    }
    // And hod reads its own text to the same text.
    let (module2, bodies2) = read(printed.as_bytes());
    let mut printed2 = String::new();
    module2
        .print(&mut printed2, Dialect::new(newest.major).expect("a dialect"), &bodies2)
        .expect("printed");
    assert!(printed == printed2, "seed {seed}: printing is not a fixed point");
}

const ORDINARY: Shape = Shape { blocks: 3000, insts: 4, huge: 0, unlabelled: 0, skips: 0 };

#[test]
fn numbered_blocks_and_values_read_as_llvm_reads_them() {
    let llvm = Llvm::all();
    let shapes = [
        // clang's shape: every block labelled, numbers in order.
        ORDINARY,
        // Numbers skipped, some by more than 4096.
        Shape { skips: 3, ..ORDINARY },
        // Blocks of more than 4096 instructions.
        Shape { blocks: 400, huge: 2, ..ORDINARY },
        // Blocks without labels (the numbering pass), with everything.
        Shape { blocks: 2000, insts: 3, huge: 1, unlabelled: 30, skips: 2 },
        Shape { blocks: 200, insts: 2, huge: 0, unlabelled: 100, skips: 0 },
    ];
    for seed in 0..4 {
        check(0x7101 + seed, &shapes, &llvm);
    }
}

// ---- refusals ----

/// `(text, what llvm-as says after "error: ")`, each checked against
/// `llvm-as` 19 and 22 too.
const REFUSED: &[(&str, &str)] = &[
    // A value number twice.
    (
        "define i32 @f(i32 %0) {\n  %2 = add i32 %0, 1\n  %2 = add i32 %0, 1\n  ret i32 %2\n}\n",
        "3:3: instruction expected to be numbered '%3' or greater",
    ),
    // A value number that goes back.
    (
        "define i32 @f(i32 %0) {\n  %3 = add i32 %0, 1\n  %2 = add i32 %0, 1\n  ret i32 %2\n}\n",
        "3:3: instruction expected to be numbered '%4' or greater",
    ),
    // A label number twice.
    (
        "define i32 @f(i32 %0) {\n  br label %2\n2:\n  br label %2\n2:\n  ret i32 %0\n}\n",
        "5:1: label expected to be numbered '3' or greater",
    ),
    // A label the unlabelled entry already took.
    (
        "define i32 @f(i32 %0) {\n  br label %1\n1:\n  br label %1\n}\n",
        "3:1: label expected to be numbered '2' or greater",
    ),
    // A label below a value's number.
    (
        "define i32 @f(i32 %0) {\n  %4 = add i32 %0, 1\n  br label %3\n3:\n  ret i32 %4\n}\n",
        "4:1: label expected to be numbered '5' or greater",
    ),
    // A value with a block's number.
    (
        "define i32 @f(i32 %0) {\n  br label %2\n2:\n  %2 = add i32 %0, 1\n  ret i32 %0\n}\n",
        "4:3: instruction expected to be numbered '%3' or greater",
    ),
    // Arguments out of order, or twice.
    (
        "define i32 @f(i32 %1, i32 %0) {\n  ret i32 %0\n}\n",
        "1:23: argument expected to be numbered '%2' or greater",
    ),
    (
        "define i32 @f(i32 %0, i32 %0) {\n  ret i32 %0\n}\n",
        "1:23: argument expected to be numbered '%1' or greater",
    ),
    // A number never defined.
    (
        "define i32 @f(i32 %0) {\n  br label %5\n2:\n  ret i32 %0\n}\n",
        "2:12: use of undefined value '%5'",
    ),
];

#[test]
fn numbers_out_of_order_are_refused_as_llvm_as_refuses_them() {
    let llvm = Llvm::all();
    for &(text, want) in REFUSED {
        assert_eq!(refusal(text.as_bytes()).as_deref(), Some(want), "{text}");
        let (place, message) = want.split_once(' ').expect("a position");
        for llvm in &llvm {
            let error = llvm.accepts(text.as_bytes()).expect_err("llvm-as refuses it");
            assert!(
                error.contains(&format!("{place} error: {message}")),
                "llvm-as {}: {error}\nfor {text}",
                llvm.major
            );
        }
    }
    // Numbers that skip are accepted by both.
    let skipped =
        "define i32 @f(i32 %0) {\n  %5 = add i32 %0, 1\n  br label %7\n7:\n  ret i32 %5\n}\n";
    assert_eq!(refusal(skipped.as_bytes()), None);
    for llvm in &llvm {
        llvm.accepts(skipped.as_bytes()).expect("llvm-as accepts skipped numbers");
    }
}
