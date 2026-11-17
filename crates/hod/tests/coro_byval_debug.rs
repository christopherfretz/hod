//! WP-342 (`spec/dwarf.md` §6.16): a coroutine whose parameter is passed
//! by address (`byval` on x86-64 and i386, a pointer to the caller's copy
//! on AArch64) and copied into the frame. clang declares the parameter
//! twice, at the incoming copy and at the frame's; hod refused it as
//! `debug:variable-twice` on x86-64 and i386 and, on AArch64, read the
//! frame copy through the incoming pointer's `DW_OP_deref` (a wrong
//! location after `coro.begin`). `CoroSplit` + `llc` read the incoming
//! copy in the ramp and the frame's in the clones; hod writes one DIE
//! that branches on the mode word to the same places.
//!
//! - `one_parameter_die_branching_on_the_mode`: clang 22's pre-split IR,
//!   three targets: one `p`, a formal parameter of `walk`, whose location
//!   is the mode branch (`DW_OP_bra`) between the incoming copy (through
//!   its address, `DW_OP_deref`) and the frame word plus the copy's
//!   offset; `llvm-dwarfdump --verify` clean.
//! - `another_expression_is_refused`: the frame copy's record with an
//!   expression of its own is `debug:coro-passed-expression`.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use hod::{CompileError, Construct, OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool};

const SOURCE: &str = r"#include <coroutine>
struct gen {
  struct promise_type {
    gen get_return_object() {
      return gen{std::coroutine_handle<promise_type>::from_promise(*this)};
    }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    void return_void() noexcept {}
    void unhandled_exception() noexcept {}
  };
  std::coroutine_handle<promise_type> h;
};
struct big { long long a, b, c, d; };
int seen(long long x) { return int(x); }
gen walk(big p) {
  co_await std::suspend_always{};
  seen(p.a + p.d);
}
";

const TRIPLES: [&str; 3] =
    ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "i686-unknown-linux-gnu"];

fn output(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

fn compile(text: &str) -> Result<Vec<u8>, CompileError> {
    let options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
}

/// clang 22's IR of [`SOURCE`] for `triple`, before LLVM's passes.
fn pre_split(clang: &Path, scratch: &Scratch, triple: &str) -> String {
    let name = &triple[..triple.find('-').unwrap()];
    let source = scratch.write(&format!("{name}.cpp"), SOURCE);
    let ir = scratch.join(format!("{name}.ll"));
    let mut command = Command::new(clang);
    command.arg(format!("--target={triple}")).args(["-std=c++20", "-g", "-O0", "-S"]);
    command.args(["-emit-llvm", "-Xclang", "-disable-llvm-passes"]);
    output(command.arg(&source).arg("-o").arg(&ir));
    std::fs::read_to_string(&ir).expect("the IR")
}

/// The metadata id of the parameter `p` (`!N`).
fn parameter_id(text: &str) -> &str {
    let line = text
        .lines()
        .find(|l| l.contains("DILocalVariable(name: \"p\", arg: "))
        .expect("the parameter's variable");
    line.split_once(" = ").expect("a metadata line").0
}

#[test]
fn one_parameter_die_branching_on_the_mode() {
    let (Some(clang), Some(dwarfdump)) = (tool("clang++-22"), tool("llvm-dwarfdump-22")) else {
        return;
    };
    let scratch = Scratch::new("coro_byval_debug");
    for triple in TRIPLES {
        let name = &triple[..triple.find('-').unwrap()];
        let text = pre_split(&clang, &scratch, triple);
        let id = parameter_id(&text);
        let var = format!(", {id}, ");
        let twice =
            text.lines().filter(|l| l.contains("#dbg_declare(ptr %") && l.contains(&var)).count();
        assert_eq!(twice, 2, "{name}: clang declares `p` twice");
        let object = scratch.write(&format!("{name}.o"), compile(&text).expect("compiles"));
        let verified = output(Command::new(&dwarfdump).arg("--verify").arg(&object));
        assert!(verified.contains("No errors."), "{name}: {verified}");
        let dies = output(Command::new(&dwarfdump).arg("--debug-info").arg(&object));
        let lines: Vec<&str> = dies.lines().collect();
        let named: Vec<usize> = (0..lines.len())
            .filter(|&i| lines[i].trim_start().starts_with("DW_AT_name\t(\"p\")"))
            .collect();
        // One DIE named `p` outside the frame type (`CoroSplit` adds
        // none: hod writes no frame type), a formal parameter.
        assert_eq!(named.len(), 1, "{name}: one `p`");
        let die = (0..named[0]).rev().find(|&i| lines[i].contains("DW_TAG_")).unwrap();
        assert!(lines[die].ends_with("DW_TAG_formal_parameter"), "{name}: {}", lines[die]);
        let location =
            lines[die..named[0]].iter().find(|l| l.contains("DW_AT_location")).expect("a location");
        // mode, deref, bra; the incoming copy's address, deref; skip; the
        // frame word, deref, plus the copy's offset.
        let ops: Vec<&str> = location
            .split(['(', ')', ','])
            .map(str::trim)
            .filter(|op| op.starts_with("DW_OP_"))
            .map(|op| op.split(' ').next().unwrap_or(op))
            .collect();
        assert_eq!(
            ops,
            [
                "DW_OP_fbreg",
                "DW_OP_deref",
                "DW_OP_bra",
                "DW_OP_fbreg",
                "DW_OP_deref",
                "DW_OP_skip",
                "DW_OP_fbreg",
                "DW_OP_deref",
                "DW_OP_plus_uconst",
            ],
            "{name}: {location}"
        );
    }
}

#[test]
fn another_expression_is_refused() {
    let Some(clang) = tool("clang++-22") else { return };
    let scratch = Scratch::new("coro_byval_refused");
    for triple in TRIPLES {
        let name = &triple[..triple.find('-').unwrap()];
        let text = pre_split(&clang, &scratch, triple);
        let var = format!(", {}, ", parameter_id(&text));
        // The frame copy's record is the last of `p`'s: give it a
        // fragment of its own.
        let at = text
            .match_indices("#dbg_declare")
            .map(|(i, _)| i)
            .filter(|&i| text[i..].lines().next().is_some_and(|l| l.contains(&var)))
            .last()
            .expect("the copy's record");
        let line = text[at..].lines().next().unwrap();
        let edited = line.replace("!DIExpression()", "!DIExpression(DW_OP_LLVM_fragment, 0, 64)");
        assert_ne!(line, edited, "{name}: {line}");
        let text = format!("{}{}{}", &text[..at], edited, &text[at + line.len()..]);
        match compile(&text) {
            Err(CompileError::Unsupported(u)) => {
                let Construct::Named(refused) = u.construct else { panic!("{:?}", u.construct) };
                assert_eq!(refused, "debug:coro-passed-expression", "{name}");
            }
            other => panic!("{name}: expected a refusal, got {:?}", other.map(|o| o.len())),
        }
    }
}
