//! `spec/ll.md` §3 and §8, snippet by snippet: what `hod_ll` accepts,
//! what it refuses and where, and what it records as unsupported, each
//! against the verdict of `llvm-as` 19 and 22 on the same text.

#[path = "../common/mod.rs"]
mod common;
mod rows;

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use hod_ir::Arch;
use hod_ll::{BodyParser, LlError, LlErrorKind, ParseOptions, ParsedModule, limits, parse_module};
use hod_testutil::{DEFAULT_TIMEOUT, run_with};

use common::{Llvm, parse};
use rows::{ROWS, UNSUPPORTED};

/// Checks that an error says where it is: the offset is in the text, and
/// the line and column are those of the offset.
fn check_position(text: &[u8], error: &LlError) {
    assert!(error.offset <= text.len(), "{error}: offset {} past the end", error.offset);
    let before = &text[..error.offset];
    let line = before.split(|&byte| byte == b'\n').count();
    let start = before.iter().rposition(|&byte| byte == b'\n').map_or(0, |at| at + 1);
    assert_eq!(error.line as usize, line, "{error}: line of offset {}", error.offset);
    assert_eq!(error.column as usize, error.offset - start + 1, "{error}: column");
    assert!(!error.message.is_empty(), "an error says what is wrong");
    let shown = error.to_string();
    assert_eq!(shown, format!("{}:{}: {}", error.line, error.column, error.message));
}

/// `text` parsed, the module level and then every body (`spec/ll.md`
/// §11): a body's error is the text's.
fn parse_all(text: &[u8]) -> Result<ParsedModule<'_>, LlError> {
    let mut parsed = parse(text, Arch::X86_64)?;
    let mut bodies = BodyParser::new(&parsed.text);
    for function in parsed.text.functions() {
        bodies.parse(&mut parsed.module, function)?;
    }
    drop(bodies);
    Ok(parsed)
}

/// What `hod_ll` makes of `text`: `ok`, or the kind of the error and its
/// line and column.
fn verdict(text: &str) -> String {
    match parse_all(text.as_bytes()) {
        Ok(_) => "ok".to_owned(),
        Err(error) => {
            check_position(text.as_bytes(), &error);
            format!("{:?}@{}:{}", error.kind, error.line, error.column)
        }
    }
}

/// What `llvm-as` makes of `text`: `+` accepted, `-` refused by the
/// parser, `v` parsed and refused by the verifier, `!` crashed. With the
/// line of the diagnostic, if it names one.
fn llvm_verdict(llvm: &Llvm, text: &str) -> (char, Option<u32>) {
    let mut command = Command::new(llvm.tool("llvm-as"));
    command.args(["-o", "/dev/null", "-"]);
    let outcome = run_with(&mut command, text.as_bytes(), DEFAULT_TIMEOUT);
    if outcome.success() {
        return ('+', None);
    }
    let stderr = outcome.stderr_text();
    if stderr.contains("does not verify") {
        return ('v', None);
    }
    let Some(rest) = stderr.split("<stdin>:").nth(1) else {
        return ('!', None);
    };
    ('-', rest.split(':').next().and_then(|line| line.parse().ok()))
}

/// Runs `job` on every index below `count` with a few threads: the
/// machine is shared.
fn each<T: Send>(count: usize, job: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let mut done: Vec<(usize, T)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= count {
                            return done;
                        }
                        done.push((index, job(index)));
                    }
                })
            })
            .collect();
        workers.into_iter().flat_map(|worker| worker.join().expect("a worker")).collect()
    });
    done.sort_by_key(|&(index, _)| index);
    done.into_iter().map(|(_, value)| value).collect()
}

/// §3, §8: every row's verdict, with the kind and the position of each
/// error.
#[test]
fn verdicts_and_error_positions() {
    let mut wrong = 0;
    for (index, &(text, mine, _, _)) in ROWS.iter().enumerate() {
        let actual = verdict(text);
        if actual != mine {
            println!("ROW\t{index}\tmine\t{actual}");
            println!("  {text:?}: expected {mine}");
            wrong += 1;
        }
    }
    assert_eq!(wrong, 0, "rows with another verdict than the table's");
}

/// §3.2, §3.5: `llvm-as` 19 and 22 give every row the verdict the table
/// says, and wherever `hod_ll` differs from either the row says why.
/// What the verifier refuses after the parser accepted it is the one
/// difference that needs no reason of its own (§3.5).
#[test]
fn llvm_as_agrees_or_the_difference_is_named() {
    let tools: Vec<Option<Llvm>> = [19, 22].map(Llvm::find).into();
    let verdicts = each(ROWS.len(), |index| {
        tools
            .iter()
            .map(|llvm| llvm.as_ref().map(|llvm| llvm_verdict(llvm, ROWS[index].0)))
            .collect::<Vec<_>>()
    });
    let (mut wrong, mut unexplained, mut differences) = (0, 0, 0);
    let (mut same_line, mut both_refuse) = (0, 0);
    for (index, &(text, mine, llvm, why)) in ROWS.iter().enumerate() {
        let accepted = mine == "ok";
        let actual: String = verdicts[index]
            .iter()
            .zip(llvm.chars())
            .map(|(verdict, expected)| verdict.map_or(expected, |(verdict, _)| verdict))
            .collect();
        if actual != llvm {
            println!("ROW\t{index}\tllvm\t{actual}");
            println!("  {text:?}: expected {llvm}");
            wrong += 1;
        }
        let differs = llvm.chars().any(|verdict| match verdict {
            '+' => !accepted,
            'v' => false,
            _ => accepted,
        });
        if differs {
            differences += 1;
            println!("differs ({mine}, llvm-as 19 and 22: {llvm}): {text:?}\n    {why}");
        }
        if differs == why.is_empty() {
            println!("  {text:?}: a reason exactly where there is a difference");
            unexplained += 1;
        }
        if !accepted {
            let line: u32 = mine
                .split('@')
                .nth(1)
                .and_then(|at| at.split(':').next())
                .map_or(0, |line| line.parse().expect("a line"));
            for (_, theirs) in verdicts[index].iter().flatten() {
                if let Some(theirs) = theirs {
                    both_refuse += 1;
                    same_line += usize::from(*theirs == line);
                }
            }
        }
    }
    println!(
        "{} rows, {differences} named differences from llvm-as; of {both_refuse} diagnostics \
         that both give, {same_line} are on the same line",
        ROWS.len()
    );
    assert_eq!(wrong, 0, "rows where llvm-as says something else than the table");
    assert_eq!(unexplained, 0, "rows with a difference and no reason, or the reverse");
}

/// §3.4: each construct `hod_ll` accepts without being able to deliver
/// it is recorded under its name, and nothing else is; `llvm-as` 19 and
/// 22 say of each row what the table says.
#[test]
fn unsupported_constructs_are_recorded_by_name() {
    let tools: Vec<Option<Llvm>> = [19, 22].map(Llvm::find).into();
    let verdicts = each(UNSUPPORTED.len(), |index| {
        tools
            .iter()
            .map(|llvm| llvm.as_ref().map(|llvm| llvm_verdict(llvm, UNSUPPORTED[index].0).0))
            .collect::<Vec<_>>()
    });
    let mut wrong = 0;
    for (index, &(text, names, llvm)) in UNSUPPORTED.iter().enumerate() {
        let mut actual: Vec<String> = match parse_all(text.as_bytes()) {
            Ok(parsed) => parsed
                .module
                .unsupported()
                .map(|unsupported| unsupported.construct.to_owned())
                .collect(),
            Err(error) => vec![format!("error: {error}")],
        };
        actual.sort_unstable();
        actual.dedup();
        if actual != names {
            println!("UNSUPPORTED\t{index}\tnames\t{actual:?}\n  {text:?}: expected {names:?}");
            wrong += 1;
        }
        let theirs: String = verdicts[index]
            .iter()
            .zip(llvm.chars())
            .map(|(verdict, expected)| verdict.unwrap_or(expected))
            .collect();
        if theirs != llvm {
            println!("UNSUPPORTED\t{index}\tllvm\t{theirs}\n  {text:?}: expected {llvm}");
            wrong += 1;
        }
    }
    assert_eq!(wrong, 0, "rows with other records or verdicts than the table's");
}

/// §8: a module that names no target needs the caller's.
#[test]
fn a_module_without_a_target_needs_a_default() {
    let error = parse_module(b"@x = global i32 0\n", &ParseOptions::default()).unwrap_err();
    assert_eq!(error.kind, LlErrorKind::Target);
    assert!(parse(b"@x = global i32 0\n", Arch::Aarch64).is_ok());
    let named = b"target triple = \"aarch64-unknown-linux-gnu\"\n@x = global i32 0\n";
    let parsed = parse_module(named, &ParseOptions::default()).expect("a target of its own");
    assert_eq!(parsed.module.target().arch(), Arch::Aarch64);
}

/// §8: nesting to the limit is accepted, one level more is a `Limit`
/// error, and far more is the same error and no stack overflow; for
/// types, for constants and for constant expressions.
#[test]
fn nesting_has_a_limit() {
    let limit = limits::MAX_NESTING as usize;
    let array = |depth: usize| {
        format!("@x = external global {}i8{}\n", "[1 x ".repeat(depth), "]".repeat(depth))
    };
    let structure = |depth: usize| {
        format!("@x = external global {}i8{}\n", "{ ".repeat(depth), " }".repeat(depth))
    };
    let expression = |depth: usize| {
        format!(
            "@y = global i8 0\n@x = global {}ptr @y{}\n",
            "ptr getelementptr (i8, ".repeat(depth),
            ", i64 1)".repeat(depth)
        )
    };
    // The type is written again at every level: the text grows with the
    // square of the depth.
    let value = |depth: usize| {
        let (mut ty, mut value) = ("i8".to_owned(), "1".to_owned());
        for _ in 0..depth.min(2 * limit) {
            value = format!("{{ {ty} {value} }}");
            ty = format!("{{ {ty} }}");
        }
        format!("@x = global {ty} {value}\n")
    };
    for (what, make) in [
        ("array types", &array as &dyn Fn(usize) -> String),
        ("struct types", &structure),
        ("constant expressions", &expression),
        ("aggregate constants", &value),
    ] {
        let mut deepest = 0;
        for depth in [1, limit / 2, limit - 1, limit, limit + 1, limit * 2, 100_000] {
            match parse(make(depth).as_bytes(), Arch::X86_64) {
                Ok(_) => {
                    assert!(depth <= limit, "{what}: depth {depth} is past the limit");
                    deepest = depth;
                }
                Err(error) => {
                    assert_eq!(error.kind, LlErrorKind::Limit, "{what} at depth {depth}: {error}");
                }
            }
        }
        println!("{what}: nested {deepest} deep at most");
        assert!(deepest >= limit / 2, "{what}: the limit is not far below MAX_NESTING");
    }
}

/// §8: an integer literal may have `MAX_INT_LITERAL_DIGITS` digits and
/// no more.
#[test]
fn integer_literals_have_a_limit() {
    let literal = |digits: usize| format!("@x = global i8388607 {}\n", "1".repeat(digits));
    assert!(parse(literal(1000).as_bytes(), Arch::X86_64).is_ok());
    let most = limits::MAX_INT_LITERAL_DIGITS;
    let error = parse(literal(most + 1).as_bytes(), Arch::X86_64).unwrap_err();
    assert_eq!(error.kind, LlErrorKind::Limit, "{error}");
    assert_eq!((error.line, error.column), (1, 22));
}

/// The blocks that the `blockaddress` constants in the initializer of
/// `@x` name, in order.
fn block_addresses(text: &[u8]) -> Vec<u32> {
    let parsed = parse_all(text).unwrap_or_else(|error| panic!("{error}"));
    let module = &parsed.module;
    let sym = module.find_symbol(b"x").expect("@x");
    let hod_ir::SymbolKind::Variable { init: Some(init), .. } = module.symbol(sym).kind() else {
        panic!("@x is a variable with an initializer");
    };
    let mut out = Vec::new();
    let mut stack = vec![init];
    while let Some(id) = stack.pop() {
        match module.consts.get(id) {
            hod_ir::Const::BlockAddr { block, .. } => out.push(block.raw()),
            hod_ir::Const::Aggregate { elems, .. } => stack.extend(elems.iter().rev()),
            other => panic!("not a block address: {other:?}"),
        }
    }
    out
}

/// §5.5: a `blockaddress` of a block without a label names the block
/// that LLVM numbers so. `llvm-as | llvm-dis` writes every block's label
/// (all but the entry block's, which a `blockaddress` cannot name), so
/// the blocks read from LLVM's text are the oracle.
#[test]
fn blockaddress_of_a_block_without_a_label_is_llvms() {
    const CASES: &[&str] = &[
        "@x = global ptr blockaddress(@f, %1)\ndefine void @f() {\n  br label %1\n\n  ret void\n}",
        "@x = global [2 x ptr] [ptr blockaddress(@f, %3), ptr blockaddress(@f, %5)]\ndefine i32 @f(i32, ptr) {\n  %a = add i32 %0, 1\n  br label %3\n\n  %4 = add i32 %a, 2\n  br label %5\n\n  ret i32 %4\n}",
        "@x = global ptr blockaddress(@f, %3)\ndeclare void @g()\ndeclare i32 @h()\ndefine void @f() {\n  call void @g()\n  call noundef i32 @h()\n  add i32 1, 2\n  br label %3\n\n  ret void\n}",
        "@x = global ptr blockaddress(@f, %4)\n%T = type { i32, i64 }\ndefine void @f(%T, %T %named, ptr byval(%T), i32 %2) {\n  tail call void @f(%T %0, %T %named, ptr byval(%T) %1, i32 %2)\n  br label %4\n\n  ret void\n}",
        "@x = global ptr blockaddress(@f, %1)\ndeclare void @g()\ndeclare i32 @p(...)\ndefine void @f() personality ptr @p {\n  invoke void @g()\n          to label %ok unwind label %1\n\n  %lp = landingpad { ptr, i32 }\n          cleanup\n          catch ptr null\n  resume { ptr, i32 } %lp\nok:\n  ret void\n}",
        "@x = global [3 x ptr] [ptr blockaddress(@f, %1), ptr blockaddress(@f, %2), ptr blockaddress(@f, %4)]\ndefine void @f(i32 %n) {\n  switch i32 %n, label %1 [\n    i32 0, label %2\n    i32 1, label %4\n  ]\n\n  ret void\n\n  %3 = add i32 %n, 1\n  ret void\n\n  ret void\n}",
        "@x = global ptr blockaddress(@f, %2)\ndeclare i32 @h()\ndeclare i32 @p(...)\ndefine void @f() personality ptr @p {\n  %1 = invoke i32 @h() to label %2 unwind label %3\n\n  ret void\n\n  %4 = landingpad { ptr, i32 } cleanup\n  ret void\n}",
        "@x = global ptr blockaddress(@f, %2)\ndefine void @f(ptr %p) {\n  %1 = atomicrmw volatile add ptr %p, i32 1 seq_cst\n  store i32 ptrtoint (ptr getelementptr inbounds (i8, ptr @x, i64 1) to i32), ptr %p\n  br label %2\n\n  ret void\n}",
    ];
    let tools: Vec<Llvm> = [19, 22].into_iter().filter_map(Llvm::find).collect();
    for text in CASES {
        let mine = block_addresses(text.as_bytes());
        for llvm in &tools {
            let mut assemble = Command::new(llvm.tool("llvm-as"));
            assemble.args(["-o", "-", "-"]);
            let bitcode = run_with(&mut assemble, text.as_bytes(), DEFAULT_TIMEOUT);
            assert!(bitcode.success(), "{text:?}: {}", bitcode.stderr_text());
            let mut disassemble = Command::new(llvm.tool("llvm-dis"));
            disassemble.args(["-o", "-", "-"]);
            let printed = run_with(&mut disassemble, &bitcode.stdout, DEFAULT_TIMEOUT);
            assert!(printed.success(), "{text:?}: {}", printed.stderr_text());
            let theirs = block_addresses(&printed.stdout);
            assert_eq!(mine, theirs, "{text:?}: LLVM {}'s blocks", llvm.major);
        }
    }
}

/// LLVM's module order as `llvm-dis` prints it: each line that defines
/// or declares a global, tagged with its kind, and the names listed kind
/// by kind (variables, functions, aliases, IFUNCs), each kind in the
/// order printed.
fn llvm_order(text: &[u8]) -> Vec<String> {
    let mut kinds: [Vec<String>; 4] = Default::default();
    for line in String::from_utf8_lossy(text).lines() {
        let name = |rest: &str| rest.split(['(', ' ']).next().unwrap_or("").to_string();
        if let Some(rest) = line.strip_prefix('@') {
            let (name, value) = rest.split_once(" = ").expect("a global's line");
            let words: Vec<&str> = value.split(' ').collect();
            let kind = if words.contains(&"alias") {
                2
            } else if words.contains(&"ifunc") {
                3
            } else {
                0
            };
            kinds[kind].push(name.to_string());
        } else if line.starts_with("define ") || line.starts_with("declare ") {
            let at = line.find(" @").expect("a function's name");
            kinds[1].push(name(&line[at + 2..]));
        }
    }
    kinds.concat()
}

/// §5.7: symbols are numbered in LLVM's module order — variables, then
/// functions, then aliases, then IFUNCs, each kind in the order of its
/// definitions and declarations in the text, whatever refers to what
/// first.
#[test]
fn symbols_are_numbered_in_llvms_module_order() {
    const TEXT: &str = "@a = global ptr @c\n@al = alias i32, ptr @b\ndefine void @f() {\n  call void @h()\n  call void @g()\n  ret void\n}\n@b = global i32 1\n@ifn = ifunc void (), ptr @resolver\ndeclare void @h()\n@c = global ptr @f\ndefine ptr @resolver() {\n  ret ptr @g\n}\n@al0 = alias i32, ptr @a\ndefine void @g() {\n  ret void\n}\n@d = global i32 0\n";
    let parsed = parse_all(TEXT.as_bytes()).unwrap_or_else(|error| panic!("{error}"));
    let mine: Vec<String> = parsed
        .module
        .symbols()
        .map(|symbol| String::from_utf8_lossy(symbol.name()).into_owned())
        .collect();
    assert_eq!(mine, ["a", "b", "c", "d", "f", "h", "resolver", "g", "al", "al0", "ifn"]);
    for llvm in [19, 22].into_iter().filter_map(Llvm::find) {
        let mut assemble = Command::new(llvm.tool("llvm-as"));
        assemble.args(["-o", "-", "-"]);
        let bitcode = run_with(&mut assemble, TEXT.as_bytes(), DEFAULT_TIMEOUT);
        assert!(bitcode.success(), "{}", bitcode.stderr_text());
        let mut disassemble = Command::new(llvm.tool("llvm-dis"));
        disassemble.args(["-o", "-", "-"]);
        let printed = run_with(&mut disassemble, &bitcode.stdout, DEFAULT_TIMEOUT);
        assert!(printed.success(), "{}", printed.stderr_text());
        assert_eq!(mine, llvm_order(&printed.stdout), "LLVM {}'s module order", llvm.major);
    }
}
