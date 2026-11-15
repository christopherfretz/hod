//! WP-261 (`spec/codegen.md` §5.6.8 "Folded constants and the blocks
//! they remove"): hod deletes the blocks `llc`'s `LowerConstantIntrinsics`
//! deletes after folding `llvm.is.constant` and `llvm.objectsize`.
//!
//! - `undefined_symbols_equal_llc`: for each shape (the simplifications,
//!   the `markAliveBlocks` rewrites, the edge cases: `blockaddress`,
//!   `indirectbr`, `callbr`, `switch`, `musttail`, an `invoke` and its
//!   landing pad), on both targets, the undefined symbols of hod's
//!   object are those of `llc` 19's and 22's;
//! - `refusals`: the four rewrites hod does not model are declined by
//!   name, and only in a function where blocks are deleted;
//! - `debug_info`: `clang-22 -g` sources whose dead arm has a line and a
//!   local: hod's object verifies, has no row at the dead line, the
//!   lines `llc` has, and no DIE for the dead local.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use hod::{CompileError, OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool, tools_required};

const TRIPLES: [&str; 2] = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"];

/// The shapes; each names undefined functions only its dead blocks call.
const SHAPES: [(&str, &str); 7] = [
    ("direct", DIRECT),
    ("simplified", SIMPLIFIED),
    ("alive", ALIVE),
    ("edges", EDGES),
    ("switch-kept", SWITCH_KEPT),
    ("musttail", MUSTTAIL),
    ("invoke", INVOKE),
];

const DIRECT: &str = r"
declare i1 @llvm.is.constant.i32(i32)
declare void @dead()
declare void @live()
define void @f(i32 %x) {
e:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %d, label %l
d:
  call void @dead()
  br label %l
l:
  call void @live()
  ret void
}
";

const SIMPLIFIED: &str = r"
declare i1 @llvm.is.constant.i32(i32)
declare i64 @llvm.objectsize.i64.p0(ptr, i1, i1, i1)
declare void @d1()
declare void @d2()
declare void @d3()
declare void @d4()
declare void @d5()
declare void @d6()
declare void @live()
define void @f(i32 %x, i1 %p, i32 %y, ptr %q) {
e:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  %n = xor i1 %c, true
  br i1 %n, label %b1, label %x1
x1:
  call void @d1()
  br label %b1
b1:
  %z = zext i1 %c to i32
  %t = icmp ne i32 %z, 0
  br i1 %t, label %x2, label %b2
x2:
  call void @d2()
  br label %b2
b2:
  %a = and i1 %c, %p
  br i1 %a, label %x3, label %b3
x3:
  call void @d3()
  br label %b3
b3:
  %s = select i1 %c, i32 1, i32 0
  %u = icmp ugt i32 %s, %y
  br i1 %u, label %x4, label %b4
x4:
  call void @d4()
  br label %b4
b4:
  %f = freeze i1 %c
  br i1 %f, label %x5, label %b5
x5:
  call void @d5()
  br label %b5
b5:
  %o = call i64 @llvm.objectsize.i64.p0(ptr %q, i1 false, i1 true, i1 false)
  %k = icmp ne i64 %o, -1
  br i1 %k, label %x6, label %b6
x6:
  call void @d6()
  br label %b6
b6:
  call void @live()
  ret void
}
";

/// Once a block is deleted: a block ends after a `noreturn` call, a
/// `br` on a constant and a `switch` on the folded value fold.
const ALIVE: &str = r"
declare i1 @llvm.is.constant.i32(i32)
declare void @nr() noreturn
declare void @u1()
declare void @u2()
declare void @u3()
declare void @u4()
define void @f(i32 %x) {
e:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %dead, label %live
dead:
  call void @u1()
  br label %live
live:
  call void @nr()
  br label %after
after:
  call void @u2()
  br i1 true, label %t, label %ff
t:
  ret void
ff:
  call void @u3()
  ret void
}
define void @g(i32 %x) {
e:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %dead, label %live
dead:
  call void @u1()
  br label %live
live:
  switch i1 %c, label %s1 [ i1 true, label %s2 ]
s1:
  ret void
s2:
  call void @u4()
  ret void
}
";

/// `blockaddress` of a deleted block, `indirectbr` and `callbr` in one.
const EDGES: &str = r#"
declare i1 @llvm.is.constant.i32(i32)
declare void @u_ba()
declare void @u_ib()
declare void @u_cb()
declare void @live()
@ba = global ptr blockaddress(@fba, %dead)
define void @fba(i32 %x) {
e:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %dead, label %l
dead:
  call void @u_ba()
  br label %l
l:
  call void @live()
  ret void
}
define void @fib(i32 %x, ptr %t) {
e:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %dead, label %l
dead:
  indirectbr ptr %t, [label %t1, label %l]
t1:
  call void @u_ib()
  br label %l
l:
  call void @live()
  ret void
}
define void @fcb(i32 %x) {
e:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %dead, label %l
dead:
  callbr void asm "", "!i"() to label %l [label %t2]
t2:
  call void @u_cb()
  br label %l
l:
  call void @live()
  ret void
}
"#;

/// No block loses its last predecessor: the `switch` on the folded
/// value keeps both arms (in `llc` too).
const SWITCH_KEPT: &str = r"
declare i1 @llvm.is.constant.i32(i32)
declare void @kept()
declare void @live()
define void @f(i32 %x, i1 %p) {
e:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %p, label %a, label %b
a:
  br i1 %c, label %b, label %b
b:
  switch i1 %c, label %l [ i1 true, label %k ]
k:
  call void @kept()
  br label %l
l:
  call void @live()
  ret void
}
";

/// A `musttail` call of a `noreturn` callee does not end its block.
const MUSTTAIL: &str = r"
declare i1 @llvm.is.constant.i32(i32)
declare void @nr(i32) noreturn
declare void @dead()
define void @f(i32 %x) {
e:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %d, label %l
d:
  call void @dead()
  br label %l
l:
  musttail call void @nr(i32 %x)
  ret void
}
";

/// An `invoke` in the dead arm: its own landing pad goes, a shared one
/// stays.
const INVOKE: &str = r"
declare i1 @llvm.is.constant.i32(i32)
declare i32 @__gxx_personality_v0(...)
declare void @dead(i32)
declare void @pad_only()
declare void @pad_shared()
declare void @thrower()
define void @own(i32 %x) personality ptr @__gxx_personality_v0 {
e:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %d, label %out
d:
  invoke void @dead(i32 %x) to label %out unwind label %pad
pad:
  %l = landingpad { ptr, i32 } catch ptr null
  call void @pad_only()
  br label %out
out:
  ret void
}
define void @shared(i32 %x) personality ptr @__gxx_personality_v0 {
e:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %d, label %live
d:
  invoke void @dead(i32 %x) to label %out unwind label %pad
live:
  invoke void @thrower() to label %out unwind label %pad
pad:
  %l = landingpad { ptr, i32 } catch ptr null
  call void @pad_shared()
  br label %out
out:
  ret void
}
";

fn module(triple: &str, body: &str) -> String {
    format!("target triple = \"{triple}\"\n{body}")
}

fn compile(text: &str) -> Result<Vec<u8>, CompileError> {
    let options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
}

fn output(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

fn undefined(nm: &Path, object: &Path) -> BTreeSet<String> {
    output(Command::new(nm).arg("-u").arg(object))
        .lines()
        .filter_map(|l| l.split_whitespace().last().map(str::to_owned))
        .collect()
}

fn llc_object(llc: &Path, triple: &str, input: &Path, out: &Path) {
    let mut command = Command::new(llc);
    command.args(["-O0", "-filetype=obj"]);
    if triple.starts_with("aarch64") {
        command.args(["-global-isel=0", "-fast-isel=0"]);
    }
    output(command.arg(input).arg("-o").arg(out));
}

#[test]
fn undefined_symbols_equal_llc() {
    let (Some(nm), Some(llc19), Some(llc22)) = (tool("llvm-nm-22"), tool("llc-19"), tool("llc-22"))
    else {
        assert!(!tools_required(), "llvm-nm-22, llc-19 and llc-22 are required");
        eprintln!("note: no llvm-nm-22, llc-19 or llc-22; skipping");
        return;
    };
    let scratch = Scratch::new("const_fold_blocks");
    for triple in TRIPLES {
        let arch = &triple[..triple.find('-').unwrap()];
        for (name, body) in SHAPES {
            let text = module(triple, body);
            let tag = format!("{name}-{arch}");
            let object = compile(&text).unwrap_or_else(|e| panic!("{tag}: {e}"));
            let ours = undefined(&nm, &scratch.write(&format!("{tag}.hod.o"), object));
            let input = scratch.write(&format!("{tag}.ll"), &text);
            for (v, llc) in [("19", &llc19), ("22", &llc22)] {
                let out = scratch.join(format!("{tag}.llc{v}.o"));
                llc_object(llc, triple, &input, &out);
                assert_eq!(ours, undefined(&nm, &out), "{tag}: hod's, then llc {v}'s");
            }
            // Each shape deletes something: no `d*`/`u*` name of a
            // deleted block survives, and the kept ones are there.
            assert!(
                !ours.iter().any(|s| s.starts_with("dead") || s.starts_with("pad_only")),
                "{tag}"
            );
            match name {
                "switch-kept" => assert!(ours.contains("kept"), "{tag}"),
                "invoke" => assert!(ours.contains("pad_shared"), "{tag}"),
                _ => {}
            }
        }
    }
}

/// A function in which a folded `br` deletes a block, followed by
/// `tail`, which `llc` would rewrite and hod does not model.
fn refused(tail: &str, decls: &str) -> String {
    format!(
        "declare i1 @llvm.is.constant.i32(i32)\ndeclare void @dead()\n\
         declare i32 @__gxx_personality_v0(...)\n{decls}\n\
         define void @f(i32 %x) personality ptr @__gxx_personality_v0 {{\ne:\n  \
         %c = call i1 @llvm.is.constant.i32(i32 %x)\n  br i1 %c, label %d, label %l\n\
         d:\n  call void @dead()\n  br label %l\nl:\n{tail}\n}}\n"
    )
}

#[test]
fn refusals() {
    let cases = [
        ("intrinsic:const-fold-null-access", "  store i32 0, ptr null\n  ret void", ""),
        (
            "intrinsic:const-fold-nounwind-invoke",
            "  invoke void @nw() to label %r unwind label %p\nr:\n  ret void\n\
             p:\n  %lp = landingpad { ptr, i32 } cleanup\n  ret void",
            "declare void @nw() nounwind",
        ),
        (
            "intrinsic:const-fold-noreturn-invoke",
            "  invoke void @nr() to label %r unwind label %p\nr:\n  ret void\n\
             p:\n  %lp = landingpad { ptr, i32 } cleanup\n  ret void",
            "declare void @nr() noreturn",
        ),
        (
            "intrinsic:const-fold-indirectbr",
            "  indirectbr ptr blockaddress(@f, %t), [label %t]\nt:\n  ret void",
            "",
        ),
    ];
    for triple in TRIPLES {
        for (name, tail, decls) in cases {
            let text = module(triple, &refused(tail, decls));
            let error = compile(&text).expect_err(name).to_string();
            assert!(error.contains(&format!("'{name}'")), "{triple}: {name}: {error}");
            // Without a deleted block (the dead arm gone), the same tail
            // is compiled as before.
            let live = text.replace("br i1 %c, label %d, label %l", "br i1 %c, label %l, label %l");
            compile(&live).unwrap_or_else(|e| panic!("{triple}: {name} without a fold: {e}"));
        }
    }
}

/// The lines of `object`'s line table that have rows (0 left out).
fn rows(dwarfdump: &Path, object: &Path) -> BTreeSet<u64> {
    output(Command::new(dwarfdump).arg("--debug-line").arg(object))
        .lines()
        .filter(|l| l.starts_with("0x"))
        .filter_map(|l| l.split_whitespace().nth(1)?.parse().ok())
        .filter(|&n| n != 0)
        .collect()
}

fn verify(dwarfdump: &Path, object: &Path) {
    let mut command = Command::new(dwarfdump);
    command.arg("--verify").arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{} --verify: {}", object.display(), outcome.stdout_text());
}

/// Line 5 is the dead arm; line 7 the live one's.
const ROWS: &str = "extern void ensure_compile_error(bool);
void ensure_error(bool) {}
int f(int x) {
  if (auto b = static_cast<bool>(x > 0); __builtin_constant_p(b)) {
    if (!b) ensure_compile_error(!b);
  } else {
    if (!b) ensure_error(!b);
  }
  return x;
}
";

/// `dead_local` is declared in the dead arm.
const LOCAL: &str = "extern void undefined_fn(int);
int g(int x) {
  if (__builtin_constant_p(x)) {
    int dead_local = x * 2;
    undefined_fn(dead_local);
  }
  int live = x + 1;
  return live;
}
";

#[test]
fn debug_info() {
    let (Some(clang), Some(llc), Some(dwarfdump)) =
        (tool("clang-22"), tool("llc-22"), tool("llvm-dwarfdump-22"))
    else {
        assert!(!tools_required(), "clang-22, llc-22 and llvm-dwarfdump-22 are required");
        eprintln!("note: no clang-22, llc-22 or llvm-dwarfdump-22; skipping");
        return;
    };
    let scratch = Scratch::new("const_fold_blocks_debug");
    for triple in TRIPLES {
        let arch = &triple[..triple.find('-').unwrap()];
        for (name, source) in [("rows", ROWS), ("local", LOCAL)] {
            let tag = format!("{name}-{arch}");
            let src = scratch.write(&format!("{tag}.cpp"), source);
            let ll = scratch.join(format!("{tag}.ll"));
            output(
                Command::new(&clang)
                    .args(["-g", "-O0", "-S", "-emit-llvm", "-fno-discard-value-names"])
                    .arg(format!("--target={triple}"))
                    .arg(&src)
                    .arg("-o")
                    .arg(&ll),
            );
            let text = std::fs::read_to_string(&ll).unwrap();
            assert!(text.contains("@llvm.is.constant"), "{tag}");
            let object = compile(&text).unwrap_or_else(|e| panic!("{tag}: {e}"));
            let ours = scratch.write(&format!("{tag}.hod.o"), object);
            let theirs = scratch.join(format!("{tag}.llc.o"));
            llc_object(&llc, triple, &ll, &theirs);
            verify(&dwarfdump, &ours);
            let (h, mut l) = (rows(&dwarfdump, &ours), rows(&dwarfdump, &theirs));
            // `spec/dwarf.md` §5.7 P1: on AArch64 `llc` keeps the
            // rewritten `br` of the `if` (line 3) as a `b` to the next
            // instruction, which hod falls through.
            if name == "local" && arch == "aarch64" {
                assert!(l.remove(&3), "{tag}: {l:?}");
            }
            assert_eq!(h, l, "{tag}: hod's lines, then llc's");
            let info = output(Command::new(&dwarfdump).arg("--debug-info").arg(&ours));
            if name == "rows" {
                assert!(!h.contains(&5) && h.contains(&7), "{tag}: {h:?}");
            } else {
                assert!(!h.contains(&4) && !h.contains(&5), "{tag}: {h:?}");
                assert!(!info.contains("\"dead_local\"") && info.contains("\"live\""), "{tag}");
            }
        }
    }
}
