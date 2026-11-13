//! Label differences in initializers on x86-64 (WP-174, `init:block-diff`,
//! `spec/codegen.md` §5.6.21): `blockaddress(@f, %a) - blockaddress(@f,
//! %b)` in a global, truncated to 16 or 32 bits or kept at 64, with a
//! number added, as computed-goto tables write it. The shared emitter
//! folds the difference into a number once the function is laid out
//! (WP-095), as the assembler folds `llc`'s `.long .Ltmp1-.Ltmp0`; the
//! table is then used by an `indirectbr` through
//! `blockaddress(@f, %base) + offset`.
//!
//! Checked against `hod_interp` and `llc -O0 -fast-isel=0` of LLVM 22
//! and 19 under the emulator (`common/float_gen.rs`), hod plain and
//! stressed (WP-146's shrinking moves the block rows the difference is
//! read from).

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use float_gen::{ll_header, parse, pic};
use hod_cg::Stress;
use hod_interp::{Interp, Program, Val};
use hod_testutil::{Scratch, run_with};

/// The arguments `@sel` is called with.
const CALLS: u64 = 64;

/// The tables: two 32-bit entries, a 64-bit one, a 16-bit one, one with
/// a number subtracted, and two negative ones (from `%two` back).
const TABLES: &str = "@tab = internal global [2 x i32] [i32 trunc (i64 sub (i64 ptrtoint (ptr blockaddress(@sel, %one) to i64), i64 ptrtoint (ptr blockaddress(@sel, %base) to i64)) to i32), i32 trunc (i64 sub (i64 ptrtoint (ptr blockaddress(@sel, %two) to i64), i64 ptrtoint (ptr blockaddress(@sel, %base) to i64)) to i32)], align 4
@tab64 = internal global i64 sub (i64 ptrtoint (ptr blockaddress(@sel, %two) to i64), i64 ptrtoint (ptr blockaddress(@sel, %base) to i64)), align 8
@tab16 = internal global i16 trunc (i64 sub (i64 ptrtoint (ptr blockaddress(@sel, %one) to i64), i64 ptrtoint (ptr blockaddress(@sel, %base) to i64)) to i16), align 2
@tabadd = internal global i64 sub (i64 sub (i64 ptrtoint (ptr blockaddress(@sel, %one) to i64), i64 ptrtoint (ptr blockaddress(@sel, %base) to i64)), i64 12), align 8
@back = internal global [2 x i32] [i32 trunc (i64 sub (i64 ptrtoint (ptr blockaddress(@sel, %base) to i64), i64 ptrtoint (ptr blockaddress(@sel, %two) to i64)) to i32), i32 trunc (i64 sub (i64 ptrtoint (ptr blockaddress(@sel, %one) to i64), i64 ptrtoint (ptr blockaddress(@sel, %two) to i64)) to i32)], align 4";

/// `@sel`: picks a table entry by `%a & 7` and jumps through it; the
/// block it lands in says which.
const SEL: &str = "define i64 @sel(i64 %a) {\nentry:\n  %j = and i64 %a, 7
  switch i64 %j, label %none [
    i64 1, label %use1
    i64 2, label %use2
    i64 3, label %use64
    i64 4, label %use16
    i64 5, label %useadd
    i64 6, label %useback
  ]
use1:
  %o1 = load i32, ptr @tab, align 4
  %s1 = sext i32 %o1 to i64
  br label %go
use2:
  %p2 = getelementptr [2 x i32], ptr @tab, i64 0, i64 1
  %o2 = load i32, ptr %p2, align 4
  %s2 = sext i32 %o2 to i64
  br label %go
use64:
  %o3 = load i64, ptr @tab64, align 8
  br label %go
use16:
  %o4 = load i16, ptr @tab16, align 2
  %s4 = sext i16 %o4 to i64
  br label %go
useadd:
  %o5 = load i64, ptr @tabadd, align 8
  %s5 = add i64 %o5, 12
  br label %go
useback:
  %k = lshr i64 %a, 3
  %k1 = and i64 %k, 1
  %p6 = getelementptr [2 x i32], ptr @back, i64 0, i64 %k1
  %o6 = load i32, ptr %p6, align 4
  %s6 = sext i32 %o6 to i64
  %t6 = getelementptr i8, ptr blockaddress(@sel, %two), i64 %s6
  br label %jump
none:
  br label %go
go:
  %off = phi i64 [ %s1, %use1 ], [ %s2, %use2 ], [ %o3, %use64 ], [ %s4, %use16 ], [ %s5, %useadd ], [ 0, %none ]
  %t = getelementptr i8, ptr blockaddress(@sel, %base), i64 %off
  br label %jump
jump:
  %to = phi ptr [ %t, %go ], [ %t6, %useback ]
  indirectbr ptr %to, [label %base, label %one, label %two]
base:
  %x0 = add i64 %a, 100
  br label %done
one:
  %x1 = mul i64 %a, 3
  br label %done
two:
  %x2 = xor i64 %a, 77
  br label %done
done:
  %r = phi i64 [ %x0, %base ], [ %x1, %one ], [ %x2, %two ]
  ret i64 %r
}
";

/// What `@sel` returns for each argument, by the interpreter.
fn interp_lines(text: &str) -> String {
    let (module, bodies) = parse(text);
    let program = Program::with_bodies(&module, &bodies).expect("a program");
    let sym = module.find_symbol(b"sel").expect("@sel");
    let mut lines = String::new();
    for i in 0..CALLS {
        let outcome = Interp::new(&program).run(sym, &[Val::int(64, u128::from(i))]);
        let Ok(Some(Val::Int { value, .. })) = outcome.result else {
            panic!("the interpreter on @sel({i}): {:?}", outcome.result);
        };
        writeln!(lines, "{}", value as u64).unwrap();
    }
    lines
}

#[test]
fn label_differences_agree_with_the_interpreter_and_llc() {
    let text = format!("{}{TABLES}\n{SEL}", ll_header());
    let want = interp_lines(&text);
    for (i, line) in want.lines().enumerate() {
        // Every entry reaches the block it names (`%one`: `3a`, `%two`:
        // `a ^ 77`, `%base`: `a + 100`), by the argument's low bits.
        let i = i as u64;
        let landed = match i & 7 {
            1 | 4 | 5 => i * 3,
            2 | 3 => i ^ 0x4d,
            6 if (i >> 3) & 1 == 1 => i * 3,
            _ => i + 100,
        };
        assert_eq!(line, landed.to_string(), "@sel({i})");
    }
    let scratch = Scratch::new("x64-block-diff");
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    let (module, bodies) = parse(&text);
    let mut objects = Vec::new();
    for (leg, stress) in [("hod-plain", Stress::default()), ("hod-stress", Stress::ALL)] {
        let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{leg}: {e}"));
        objects.push((leg.to_owned(), scratch.write(&format!("{leg}.o"), object)));
    }
    for major in [22, 19] {
        if let Some(object) = float_gen::llc(&scratch, major, &format!("llc{major}"), &text) {
            objects.push((format!("llc{major}"), object));
        }
    }
    let driver = toolchain.compile_c(
        &scratch,
        "driver",
        &format!(
            "#include <stdio.h>\nlong sel(long);\nint main(void) {{\n  \
             for (long i = 0; i < {CALLS}; i++) printf(\"%lu\\n\", (unsigned long)sel(i));\n  \
             return 0;\n}}\n"
        ),
        &[],
    );
    let mut ran = Vec::new();
    for (leg, object) in &objects {
        for linker in &toolchain.linkers {
            let program = scratch.join(format!("{leg}-{}", linker.name));
            let inputs: [&Path; 2] = [&driver, object];
            toolchain.link_ok(linker, &[], &inputs, &program);
            let outcome =
                run_with(&mut toolchain.runner_command(&program), &[], Duration::from_mins(2));
            assert!(outcome.success(), "{leg}/{}: {}", linker.name, outcome.stderr_text());
            assert_eq!(outcome.stdout_text(), want, "{leg}/{}", linker.name);
            ran.push(format!("{leg}/{}", linker.name));
        }
    }
    eprintln!("x64 block-diff: {CALLS} calls agree with the interpreter over {ran:?}");
}
