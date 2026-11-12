//! `spec/ll.md` §3 and §8, snippet by snippet: what `hod_ll` accepts,
//! what it refuses and where, and what it records as unsupported, each
//! against the verdict of `llvm-as` 19 and 22 on the same text.

#[path = "../common/mod.rs"]
mod common;
mod rows;

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use hod_ir::Arch;
use hod_ll::{LlError, LlErrorKind, ParseOptions, limits, parse_module};
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

/// What `hod_ll` makes of `text`: `ok`, or the kind of the error and its
/// line and column.
fn verdict(text: &str) -> String {
    match parse(text.as_bytes(), Arch::X86_64) {
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
        let mut actual: Vec<String> = match parse(text.as_bytes(), Arch::X86_64) {
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
