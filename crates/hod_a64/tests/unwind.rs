//! Native unwinding through hod-compiled frames (WP-049, `spec/eh.md`
//! §1.1, `spec/codegen.md` §9.7): `uwtable` leaf functions with frames of
//! several sizes, the largest a megabyte, compiled by hod (and once more
//! in far-branch mode) and linked by every linker, trap in their shared
//! trap block; the stack is then walked through the `.eh_frame` that
//! `emit_module` wrote, by `_Unwind_Backtrace` from the `SIGTRAP` handler
//! and by `gdb`'s `backtrace`. Each walk must go from the leaf to the C
//! caller to `main`.
//!
//! A C++ exception thrown through a hod-compiled frame that calls an
//! external function needs calls (WP-038); its test is written and
//! ignored until then.

mod common;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use common::{emit, ll_header, llc_major, parse, write_object};
use hod_cg::{Options, Stress};
use hod_testutil::{Scratch, run_with, tool, tools_required};

/// The body slot of each leaf, in bytes: none, one word, beyond a 12-bit
/// immediate, and a megabyte.
const SLOTS: [u64; 4] = [0, 16, 4112, 1 << 20];

/// `leaf_<i>()`: a frame with a slot of `SLOTS[i]` bytes, and a trap.
/// (A conditional branch, for a leaf that also returns, is WP-035's.)
fn leaves(major: u32) -> String {
    let mut text = ll_header(major);
    for (i, slot) in SLOTS.iter().enumerate() {
        let alloca = if *slot == 0 {
            String::new()
        } else {
            format!("  %s = alloca [{slot} x i8], align 16\n")
        };
        let _ =
            writeln!(text, "define i32 @leaf_{i}() uwtable {{\nentry:\n{alloca}  unreachable\n}}");
    }
    text
}

/// The C side: `outer` calls the leaf `argv[1]` names; the `SIGTRAP`
/// handler walks the stack and prints the name of each frame's function
/// (`dladdr` of the address before the return address, which for the
/// trapping frame is inside the leaf too), then exits.
const DRIVER: &str = r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <unwind.h>

int leaf_0(void), leaf_1(void), leaf_2(void), leaf_3(void);
static int (*const leaves[])(void) = { leaf_0, leaf_1, leaf_2, leaf_3 };

static _Unwind_Reason_Code step(struct _Unwind_Context *context, void *arg) {
    int *depth = arg;
    Dl_info info;
    unsigned long ip = _Unwind_GetIP(context);
    if (ip != 0 && dladdr((void *)(ip - 1), &info) && info.dli_sname)
        printf("%s ", info.dli_sname);
    else
        printf("? ");
    return ++*depth < 32 ? _URC_NO_REASON : _URC_END_OF_STACK;
}

static void on_trap(int sig) {
    int depth = 0;
    (void)sig;
    _Unwind_Backtrace(step, &depth);
    printf("\n");
    fflush(stdout);
    _exit(0);
}

__attribute__((noinline)) int outer(int which) {
    int r = leaves[which]();
    __asm__ volatile("" ::: "memory");
    return r + 1;
}

int main(int argc, char **argv) {
    int which = argc > 1 ? atoi(argv[1]) : 0;
    if (argc < 3) signal(SIGTRAP, on_trap);
    return outer(which);
}
"#;

fn run(program: &Path, args: &[&str]) -> hod_testutil::Outcome {
    let mut command = Command::new(program);
    command.args(args);
    run_with(&mut command, &[], Duration::from_mins(1))
}

/// Whether the walk `line` goes leaf, `outer`, `main` in that order.
fn walks(line: &str, leaf: &str) -> bool {
    let names: Vec<&str> = line.split_whitespace().collect();
    names.windows(3).any(|w| w == [leaf, "outer", "main"])
}

/// The programs, one per linker and mode: (description, path).
fn programs(scratch: &Scratch, toolchain: &Toolchain) -> Vec<(String, PathBuf)> {
    let (module, bodies) = parse(&leaves(llc_major()));
    let driver =
        toolchain.compile_c(scratch, "driver", DRIVER, &["-O0", "-fasynchronous-unwind-tables"]);
    let mut out = Vec::new();
    for (mode, stress) in
        [("near", Stress::default()), ("far", Stress { far: true, ..Stress::default() })]
    {
        let options = Options { stress, ..Options::default() };
        let emitted = emit(&module, &bodies, &options);
        let object = write_object(scratch, &format!("leaves-{mode}.o"), &emitted);
        for linker in &toolchain.linkers {
            let program = scratch.join(format!("unwind-{mode}-{}", linker.name));
            toolchain.link_ok(
                linker,
                &["-rdynamic"],
                &[&driver, &object, Path::new("-ldl")],
                &program,
            );
            out.push((format!("{mode}/{}", linker.name), program));
        }
    }
    out
}

fn host_toolchain(scratch: &Scratch) -> Option<Toolchain> {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: not an AArch64 host; skipping");
        return None;
    }
    Toolchain::host(scratch)
}

/// `_Unwind_Backtrace` from the `SIGTRAP` handler, through the signal
/// frame, the leaf and its caller.
#[test]
fn unwind_backtrace_walks_out_of_hod_leaves() {
    let scratch = Scratch::new("a64-unwind");
    let Some(toolchain) = host_toolchain(&scratch) else { return };
    let mut walked = 0;
    for (what, program) in programs(&scratch, &toolchain) {
        for i in 0..SLOTS.len() {
            let ran = run(&program, &[&i.to_string()]);
            let line = ran.stdout_text();
            assert_eq!(exit_code(&ran), Some(0), "{what} leaf_{i}: {line}{}", ran.stderr_text());
            let leaf = format!("leaf_{i}");
            assert!(walks(&line, &leaf), "{what}: the walk from {leaf}: {line}");
            walked += 1;
        }
    }
    eprintln!("unwind: _Unwind_Backtrace left {walked} hod frames");
}

/// `gdb`'s `backtrace` at the trap, from `.eh_frame` alone (the leaves
/// have no debug information).
#[test]
fn gdb_backtraces_through_hod_leaves() {
    let scratch = Scratch::new("a64-unwind-gdb");
    let Some(toolchain) = host_toolchain(&scratch) else { return };
    let Some(gdb) = tool("gdb") else { return };
    let mut traced = 0;
    for (what, program) in programs(&scratch, &toolchain) {
        for i in 0..SLOTS.len() {
            let mut command = Command::new(&gdb);
            command.args(["-nx", "-batch", "-ex", "run", "-ex", "bt", "--args"]);
            command.arg(&program).args([&i.to_string(), "under-gdb"]);
            let ran = run_with(&mut command, &[], Duration::from_mins(2));
            let text = format!("{}{}", ran.stdout_text(), ran.stderr_text());
            if text.contains("ptrace: Operation not permitted") {
                assert!(!tools_required(), "gdb cannot trace here (HOD_REQUIRE_TOOLS=1)");
                eprintln!("note: gdb cannot trace in this container; skipping");
                return;
            }
            let frames: Vec<&str> = text
                .lines()
                .filter(|l| l.starts_with('#'))
                .filter_map(|l| {
                    let name = l.split(" in ").nth(1).or_else(|| l.split_whitespace().nth(1))?;
                    name.split_whitespace().next()
                })
                .collect();
            let leaf = format!("leaf_{i}");
            assert!(
                frames.windows(3).any(|w| w == [leaf.as_str(), "outer", "main"]),
                "{what}: gdb's backtrace from {leaf}: {frames:?}\n{text}"
            );
            assert!(!text.contains("Backtrace stopped"), "{what} {leaf}:\n{text}");
            traced += 1;
        }
    }
    eprintln!("unwind: gdb traced out of {traced} hod frames");
}

/// A C++ exception thrown by an external function, through a hod frame
/// that calls it, into a C++ handler, under every linker. Needs calls.
#[test]
fn a_throw_passes_through_a_hod_frame() {
    let scratch = Scratch::new("a64-unwind-throw");
    let Some(toolchain) = host_toolchain(&scratch) else { return };
    let text = format!(
        "{}declare void @thrower(i32)\n\
         define i32 @through(i32 %x) uwtable {{\n  call void @thrower(i32 %x)\n  ret i32 1\n}}\n",
        ll_header(llc_major())
    );
    let (module, bodies) = parse(&text);
    let object = write_object(&scratch, "through.o", &emit(&module, &bodies, &Options::default()));
    let main = toolchain.compile_c(
        &scratch,
        "main",
        "#include <cstdio>\nextern \"C\" int through(int);\n\
         extern \"C\" void thrower(int x) { if (x) throw x; }\n\
         int main() { try { through(5); } catch (int e) { std::printf(\"caught %d\\n\", e); return 0; }\n\
         return 1; }\n",
        &["-x", "c++"],
    );
    for linker in &toolchain.linkers {
        let program = scratch.join(format!("throw-{}", linker.name));
        let ran =
            toolchain.link_and_run(linker, &[], &[&main, &object, Path::new("-lstdc++")], &program);
        assert_eq!(exit_code(&ran), Some(0), "{}: {}", linker.name, ran.stderr_text());
        assert_eq!(ran.stdout_text(), "caught 5\n", "{}", linker.name);
    }
}
