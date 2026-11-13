//! The x86-64 integer group (WP-122, `spec/codegen.md` §5.6 "Integers
//! (WP-122)"): binary operations, casts, `icmp`, `select`, `freeze` on
//! `Gpr`-class values at every width, checked against `hod_interp` and
//! `llc -O0 -fast-isel=0` (LLVM 22 and 19) under the emulator
//! (`common/int_gen.rs`); the compare fusions and the flags rule on the
//! disassembly; the short forms; what is refused, by name.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/int_gen.rs"]
mod int_gen;

use std::process::Command;
use std::time::{Duration, Instant};

use hod_cg::{CgError, Stress};
use hod_testutil::{Scratch, run_with, tool};
use hod_x64::int;
use int_gen::{Report, Rng, Suite, assert_agrees, ll_header, parse, pic, run};

/// Prints what a suite's run measured.
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn summary(name: &str, suite: &Suite, report: &Report) {
    let per_inst = |bytes: usize| bytes as f64 / report.insts.max(1) as f64;
    eprintln!(
        "x64 int {name}: {} functions, {} cases compared on {:?}, {} undefined (not run); \
         {} IR instructions compiled in {:?} ({:.0} ns each, {}); \
         code: hod {:.2} bytes per IR instruction, llc 22 {:.2} (hod/llc {:.2}); \
         interpreter {:?}, tools {:?}",
        suite.funcs.len(),
        report.compared,
        report.legs,
        report.undefined,
        report.insts,
        report.compile,
        report.compile.as_nanos() as f64 / report.insts.max(1) as f64,
        if cfg!(debug_assertions) { "debug" } else { "release" },
        per_inst(report.hod_bytes),
        per_inst(report.llc_bytes),
        report.hod_bytes as f64 / report.llc_bytes.max(1) as f64,
        report.interp,
        report.tools,
    );
}

fn check(name: &str, suite: &Suite) {
    let report = run(name, suite);
    summary(name, suite, &report);
    assert_agrees(suite, &report);
}

/// Every binary operation at every width, register and constant
/// operands on either side, edge values (x86's immediate boundaries
/// included).
#[test]
fn every_binary_operation_at_every_width_agrees() {
    let suite = int_gen::binary_table(&mut Rng(122));
    check("binary", &suite);
}

/// Every predicate at every width and on pointers, as a value and fused
/// into `select`, `zext` and `sext`; `select` and `freeze`.
#[test]
fn every_compare_and_select_agrees() {
    let suite = int_gen::compare_table(&mut Rng(123));
    check("compare", &suite);
}

/// Every cast between the widths and between integers and pointers
/// (`ptrtoaddr` with `llc` 22 only: 19 does not know it).
#[test]
fn every_cast_agrees() {
    let suite = int_gen::cast_table(&mut Rng(124), true);
    check("cast", &suite);
}

/// Random straight-line integer functions (with `br`s between blocks).
/// `HOD_INT_FUNCTIONS=<n>` sets how many (default 1500).
#[test]
fn random_integer_functions_agree() {
    let count =
        std::env::var("HOD_INT_FUNCTIONS").ok().and_then(|text| text.parse().ok()).unwrap_or(1500);
    let suite = int_gen::random_functions(&mut Rng(0x1220), count);
    check("random", &suite);
}

// ---- the code itself ----

/// The object of `functions` (the header added), compiled
/// position-independent with `stress`.
fn object(functions: &str, stress: Stress) -> Vec<u8> {
    let (m, bodies) = parse(&format!("{}{functions}", ll_header()));
    hod_x64::compile_to_object(&m, &bodies, &pic(stress)).unwrap_or_else(|e| panic!("{e}"))
}

/// `llvm-objdump -d -M intel` of `functions`: one line per instruction,
/// `mnemonic operands`, or one `<name>:` line per function. `None`
/// without the tool.
fn disassembly(functions: &str, stress: Stress) -> Option<Vec<String>> {
    let objdump = tool("llvm-objdump")?;
    let scratch = Scratch::new("x64-int-disasm");
    let path = scratch.write("f.o", object(functions, stress));
    let mut command = Command::new(objdump);
    command.args(["-d", "-M", "intel", "--no-show-raw-insn", "--no-leading-addr"]).arg(&path);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    Some(
        outcome
            .stdout_text()
            .lines()
            .filter_map(|line| {
                if line.ends_with(">:") {
                    return Some(line.to_string());
                }
                if !line.starts_with(char::is_whitespace) || line.trim().is_empty() {
                    return None;
                }
                let line = line.split('#').next().unwrap_or(line);
                Some(line.split_whitespace().collect::<Vec<_>>().join(" "))
            })
            .collect(),
    )
}

/// The mnemonic of a disassembled line.
fn mnemonic(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or("")
}

/// Whether `m` reads the flags: `setcc`, `cmovcc`, `jcc`.
fn reads_flags(m: &str) -> bool {
    (m.starts_with("set") || m.starts_with("cmov") || (m.starts_with('j') && m != "jmp"))
        && m != "jmpq"
}

/// What may run between a compare and its reader (§5.6 "Flags").
fn keeps_flags(m: &str) -> bool {
    matches!(m, "mov" | "movabs" | "lea" | "movzx" | "movsx" | "movsxd" | "push" | "pop")
}

/// §5.6 "Flags" (design 0006 §2.3): in every compare-and-select
/// function, plain and under every stress option (spills and reloads
/// between the compare and its reader), each `setcc`/`cmovcc` is reached
/// from a `cmp` or `test` through flag-keeping glue only.
#[test]
fn every_fused_pair_keeps_the_flags() {
    let suite = int_gen::compare_table(&mut Rng(125));
    for stress in [Stress::default(), Stress::ALL] {
        let Some(lines) = disassembly(&suite.text, stress) else { return };
        let mut readers = 0;
        for (at, line) in lines.iter().enumerate() {
            if !reads_flags(mnemonic(line)) {
                continue;
            }
            readers += 1;
            let setter = lines[..at]
                .iter()
                .rev()
                .find(|l| !keeps_flags(mnemonic(l)))
                .map_or("", |l| mnemonic(l));
            assert!(
                matches!(setter, "cmp" | "test"),
                "{line} is reached from {setter}:\n{}",
                lines[at.saturating_sub(12)..=at].join("\n")
            );
        }
        assert!(readers > 1000, "{readers} readers");
    }
}

/// §4.6: a compare whose only use is the next `select` is fused into it:
/// `cmp` then `cmov`, no `setcc`; `zext` and `sext` of a compare are
/// `setcc` + `movzx` (+ `neg`); a compare used elsewhere is `cmp` +
/// `setcc`, and the `select` on it tests bit 0.
#[test]
fn compares_fuse_into_select_zext_and_sext() {
    let Some(fused) = disassembly(
        "define i64 @f(i64 %a, i64 %b) {\n  %c = icmp slt i64 %a, %b\n  \
         %r = select i1 %c, i64 %a, i64 %b\n  ret i64 %r\n}\n",
        Stress::default(),
    ) else {
        return;
    };
    let has = |lines: &[String], m: &str| lines.iter().any(|l| mnemonic(l) == m);
    let starts = |lines: &[String], m: &str| lines.iter().any(|l| mnemonic(l).starts_with(m));
    assert!(has(&fused, "cmp") && has(&fused, "cmovl"), "{fused:#?}");
    assert!(!starts(&fused, "set") && !has(&fused, "test"), "{fused:#?}");

    let zext = disassembly(
        "define i32 @f(i32 %a) {\n  %c = icmp ult i32 %a, 10\n  %r = zext i1 %c to i32\n  \
         ret i32 %r\n}\n",
        Stress::default(),
    )
    .unwrap();
    assert!(zext.iter().any(|l| l == "cmp edi, 0xa") && has(&zext, "setb"), "{zext:#?}");
    let sext = disassembly(
        "define i64 @f(i64 %a) {\n  %c = icmp eq i64 %a, -5\n  %r = sext i1 %c to i64\n  \
         ret i64 %r\n}\n",
        Stress::default(),
    )
    .unwrap();
    assert!(sext.iter().any(|l| l == "cmp rdi, -0x5") && has(&sext, "sete"), "{sext:#?}");
    assert!(has(&sext, "neg"), "{sext:#?}");

    let apart = disassembly(
        "define i64 @f(i64 %a, i64 %b, i64 %x) {\n  %c = icmp slt i64 %a, %b\n  \
         %y = add i64 %x, 1\n  %r = select i1 %c, i64 %x, i64 %y\n  ret i64 %r\n}\n",
        Stress::default(),
    )
    .unwrap();
    assert!(has(&apart, "setl") && has(&apart, "test") && has(&apart, "cmovne"), "{apart:#?}");
}

/// The short forms, instruction for instruction: an immediate in place;
/// `not` for `xor` with all ones; a constant `lshr` of an odd width as
/// `shl` + `shr`; a register shift through `cl`; a division through
/// `eax`/`edx`; an `i8` compare of registers by `movsx`; an `i17` one by
/// shifting both operands; `select` as a 32-bit `cmov` for `i8`.
#[test]
fn narrow_forms_are_short() {
    let one = |text: &str| disassembly(text, Stress::default());
    let Some(add) = one("define i32 @f(i32 %a) {\n  %r = add i32 %a, 4096\n  ret i32 %r\n}\n")
    else {
        return;
    };
    assert!(add.contains(&"add edi, 0x1000".to_string()), "{add:#?}");
    let add48 = one("define i48 @f(i48 %a) {\n  %r = add i48 %a, -1\n  ret i48 %r\n}\n").unwrap();
    assert!(add48.contains(&"add rdi, -0x1".to_string()), "{add48:#?}");
    let not = one("define i64 @f(i64 %a) {\n  %r = xor i64 %a, -1\n  ret i64 %r\n}\n").unwrap();
    assert!(not.contains(&"not rdi".to_string()), "{not:#?}");
    let lshr = one("define i17 @f(i17 %a) {\n  %r = lshr i17 %a, 3\n  ret i17 %r\n}\n").unwrap();
    assert!(lshr.contains(&"shl edi, 0xf".to_string()), "{lshr:#?}");
    assert!(lshr.contains(&"shr edi, 0x12".to_string()), "{lshr:#?}");
    let shl =
        one("define i64 @f(i64 %a, i64 %b) {\n  %r = shl i64 %a, %b\n  ret i64 %r\n}\n").unwrap();
    assert!(shl.contains(&"mov rcx, rsi".to_string()), "{shl:#?}");
    assert!(shl.contains(&"shl rdi, cl".to_string()), "{shl:#?}");
    let div =
        one("define i32 @f(i32 %a, i32 %b) {\n  %r = sdiv i32 %a, %b\n  ret i32 %r\n}\n").unwrap();
    for want in ["mov rax, rdi", "cdq", "idiv esi", "mov esi, eax"] {
        assert!(div.contains(&want.to_string()), "{want}: {div:#?}");
    }
    let urem =
        one("define i64 @f(i64 %a, i64 %b) {\n  %r = urem i64 %a, %b\n  ret i64 %r\n}\n").unwrap();
    for want in ["mov edx, 0x0", "div rsi", "mov rsi, rdx"] {
        assert!(urem.contains(&want.to_string()), "{want}: {urem:#?}");
    }
    let cmp =
        one("define i1 @f(i8 %a, i8 %b) {\n  %r = icmp sgt i8 %a, %b\n  ret i1 %r\n}\n").unwrap();
    for want in ["movsx edi, dil", "movsx esi, sil", "cmp edi, esi", "setg"] {
        assert!(cmp.iter().any(|l| l.starts_with(want)), "{want}: {cmp:#?}");
    }
    let odd = one("define i1 @f(i17 %a, i17 %b) {\n  %r = icmp ult i17 %a, %b\n  ret i1 %r\n}\n")
        .unwrap();
    for want in ["shl edi, 0xf", "shl esi, 0xf", "cmp edi, esi"] {
        assert!(odd.contains(&want.to_string()), "{want}: {odd:#?}");
    }
    let select = one(
        "define i8 @f(i1 %c, i8 %a, i8 %b) {\n  %r = select i1 %c, i8 %a, i8 %b\n  ret i8 %r\n}\n",
    )
    .unwrap();
    for want in ["test dil, 0x1", "cmovne ebx, esi"] {
        assert!(select.contains(&want.to_string()), "{want}: {select:#?}");
    }
}

/// What the group does not lower, by name: `addrspacecast`
/// (`classify` rejects its non-zero address spaces first). Everything
/// else of the group compiles.
#[test]
fn what_is_not_lowered_is_refused_by_name() {
    let text = format!(
        "{}define i64 @f(ptr %p) {{\n  %q = addrspacecast ptr %p to ptr addrspace(1)\n  \
         %r = ptrtoint ptr addrspace(1) %q to i64\n  ret i64 %r\n}}\n",
        ll_header()
    );
    let (m, bodies) = parse(&text);
    match hod_x64::compile_to_object(&m, &bodies, &pic(Stress::default())) {
        Err(hod_x64::ModuleError::Function { error, .. }) => {
            assert_eq!(error, CgError::unsupported(int::ADDRSPACECAST));
        }
        other => panic!("not refused: {:?}", other.map(|o| o.len())),
    }
    object("define i64 @f(i64 %a) {\n  %r = add i64 %a, 1\n  ret i64 %r\n}\n", Stress::default());
}

/// §11 (information): nanoseconds per IR instruction of the random
/// functions, with a warm worker, and bytes of code per IR instruction.
#[test]
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn nanoseconds_per_integer_instruction() {
    let suite = int_gen::random_functions(&mut Rng(0x1221), 400);
    let (m, bodies) = parse(&format!("{}{}", ll_header(), suite.text));
    let insts: usize = bodies.iter().map(hod_ir::Body::inst_count).sum();
    let options = pic(Stress::default());
    let mut scratch = hod_cg::Scratch::new();
    let rounds = if cfg!(debug_assertions) { 3 } else { 30 };
    let mut bytes = 0;
    let start = Instant::now();
    for _ in 0..rounds {
        let mut chunk = hod_cg::Chunk::new();
        for body in &bodies {
            hod_cg::compile_function(&hod_x64::X64, &m, body, &options, &mut scratch, &mut chunk)
                .unwrap();
        }
        bytes = chunk.code.len();
    }
    let ns = start.elapsed().as_nanos() as f64 / (rounds * insts) as f64;
    let build = if cfg!(debug_assertions) { "debug" } else { "release" };
    eprintln!(
        "x64 int: {ns:.0} ns per IR instruction ({build}, {insts} instructions, {rounds} rounds); \
         {:.2} bytes of code per IR instruction",
        bytes as f64 / insts as f64
    );
}
