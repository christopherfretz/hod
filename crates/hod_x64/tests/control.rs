//! The x86-64 control group (WP-124, `spec/codegen.md` §6, §5.6.3):
//! `condbr` on an `i1` and on a fused `icmp`, `switch` by
//! `hod_cg::switch`'s plan (and limb by limb for `Mem`-class
//! conditions), `indirectbr`, `blockaddress` in code and in data, the
//! shared trap of unreachable blocks, and the `phi` protocol on the real
//! target. The oracle is `hod_interp`; the second one `llc -O0
//! -fast-isel=0` of LLVM 19 and 22. Thousands of functions per object,
//! one emulated process per object (`common::control_gen::check`).
//!
//! `HOD_CONTROL_SEEDS=<n>` sets the batches of `many_graphs_agree`.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/control_gen.rs"]
mod graphs;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use graphs::{
    Call, Func, GenOptions, Mode, Rng, Ty, Wrong, check, function, llc, mask, parse, signed,
    text_of,
};
use hod_cg::{CgError, Chunk, Options, RelocModel, Scratch as CgScratch, Stress, compile_function};
use hod_interp::{Interp, Program, Val};
use hod_testutil::{Scratch, run_with, tool};
use hod_x64::{ModuleError, X64};
use linking::{Toolchain, exit_code};

/// Position-independent, as the corpus is compiled.
fn default_options() -> Options {
    Options { reloc_model: RelocModel::Pic, ..Options::default() }
}

/// Plain, and every stress option (x86-64 has no far mode: `far` changes
/// nothing, §5.6).
fn modes() -> [Mode; 2] {
    [
        Mode { name: "plain", options: default_options() },
        Mode { name: "stress", options: Options { stress: Stress::ALL, ..default_options() } },
    ]
}

/// Panics with a report of the first disagreements.
fn assert_agree(wrong: &[Wrong], funcs: &[Func]) {
    if let Some(first) = wrong.first() {
        let mut report = String::new();
        for w in wrong.iter().take(8) {
            writeln!(
                report,
                "{} call {} @{}: interpreter {} native {}",
                w.mode, w.call, w.func, w.want, w.got
            )
            .unwrap();
        }
        panic!("{} disagreements\n{report}\n{}", wrong.len(), text_of(funcs, &first.func));
    }
}

/// Compiles every body of `functions` into one chunk.
fn chunk(functions: &str, options: &Options) -> Result<Chunk, CgError> {
    let (m, bodies) = parse(functions);
    let mut scratch = CgScratch::new();
    let mut chunk = Chunk::new();
    for body in &bodies {
        compile_function(&X64, &m, body, options, &mut scratch, &mut chunk)?;
    }
    Ok(chunk)
}

/// The first refusal of `functions`, or `Ok`.
fn compile_ll(functions: &str) -> Result<(), CgError> {
    chunk(functions, &default_options()).map(|_| ())
}

/// The object of `functions` compiled by hod.
fn object(functions: &str, options: &Options) -> Result<Vec<u8>, ModuleError> {
    let (m, bodies) = parse(functions);
    hod_x64::compile_to_object(&m, &bodies, options)
}

/// `llvm-objdump -d` (Intel syntax) of an object: per function, its
/// instructions as `mnemonic operands`.
fn disassemble(object: &Path) -> Option<Vec<(String, Vec<String>)>> {
    let objdump = tool("llvm-objdump")?;
    let mut command = Command::new(objdump);
    command
        .args(["-d", "--no-show-raw-insn", "--no-leading-addr", "--x86-asm-syntax=intel"])
        .arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    let mut funcs: Vec<(String, Vec<String>)> = Vec::new();
    for line in outcome.stdout_text().lines() {
        if let Some(name) = line.strip_prefix('<').and_then(|l| l.strip_suffix(">:")) {
            funcs.push((name.to_owned(), Vec::new()));
        } else if line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
            let text = line.split('#').next().unwrap_or(line);
            if let Some((_, insts)) = funcs.last_mut() {
                insts.push(text.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }
    }
    Some(funcs)
}

/// The mnemonic of a disassembled instruction.
fn mnemonic(inst: &str) -> &str {
    inst.split_whitespace().next().unwrap_or("")
}

/// Whether `m` is a conditional jump.
fn is_jcc(m: &str) -> bool {
    m.starts_with('j') && m != "jmp"
}

/// The flags rule (design 0006 §2.3, §5.6): walking back from every
/// conditional jump over other conditional jumps and the flag-keeping
/// glue (`mov`, `movabs`, `lea`, `movzx`, `movsx`, `push`, `pop`), the
/// first instruction is a `cmp` or a `test`. Returns the violations.
fn flags_violations(funcs: &[(String, Vec<String>)]) -> Vec<String> {
    const GLUE: [&str; 8] = ["mov", "movabs", "lea", "movzx", "movsx", "movsxd", "push", "pop"];
    let mut bad = Vec::new();
    for (name, insts) in funcs {
        for (i, inst) in insts.iter().enumerate() {
            if !is_jcc(mnemonic(inst)) {
                continue;
            }
            let writer = insts[..i]
                .iter()
                .rev()
                .map(|s| mnemonic(s))
                .find(|m| !is_jcc(m) && !GLUE.contains(m));
            if !matches!(writer, Some("cmp" | "test")) {
                bad.push(format!("{name}: `{inst}` after {writer:?}"));
            }
        }
    }
    bad
}

/// Writes `bytes` and disassembles them.
fn disassemble_bytes(
    dir: &Scratch,
    name: &str,
    bytes: &[u8],
) -> Option<Vec<(String, Vec<String>)>> {
    disassemble(&dir.write(name, bytes))
}

// ---- random graphs (WP-037's generator) ----

/// `count` functions of `seed` with `per` calls each.
fn batch(seed: u64, count: usize, per: usize, o: &GenOptions) -> (Vec<Func>, Vec<Call>) {
    let mut rng = Rng(seed);
    let funcs: Vec<Func> =
        (0..count).map(|i| function(&mut rng, &format!("g{seed}_{i}"), o)).collect();
    let mut calls = Vec::new();
    for (i, f) in funcs.iter().enumerate() {
        for args in f.arg_sets(&mut rng, per) {
            calls.push(Call { func: i, args });
        }
    }
    (funcs, calls)
}

/// Checks `batches` batches from `first`; returns the functions checked.
fn check_graphs(first: u64, batches: u64, count: usize, o: &GenOptions, with_llc: bool) -> usize {
    let mut checked = 0;
    for seed in first..first + batches {
        let (funcs, calls) = batch(seed, count, 6, o);
        let Some(wrong) = check("x64-control", &funcs, &calls, &modes(), with_llc, seed) else {
            return checked;
        };
        assert_agree(&wrong, &funcs);
        checked += funcs.len();
    }
    checked
}

/// Random graphs with `undef` and `poison` incoming values, against the
/// interpreter; then graphs without them, against `llc` 19 and 22 as
/// well. 1000 functions per object.
#[test]
fn random_graphs_agree_with_the_interpreter_and_llc() {
    let start = Instant::now();
    let a = check_graphs(1, 2, 1000, &GenOptions::default(), false);
    let o = GenOptions { undef: false, ..GenOptions::default() };
    let b = check_graphs(101, 1, 1000, &o, true);
    eprintln!("x64 control: {a} + {b} (with llc) graphs agree in {:?}", start.elapsed());
}

/// Many more graphs.
#[test]
#[ignore = "minutes under emulation; run with --ignored"]
fn many_graphs_agree() {
    let batches: u64 =
        std::env::var("HOD_CONTROL_SEEDS").ok().and_then(|text| text.parse().ok()).unwrap_or(20);
    let start = Instant::now();
    let checked = check_graphs(1_000, batches, 1000, &GenOptions::default(), false);
    eprintln!("x64 control: {checked} graphs agree in {:?}", start.elapsed());
}

/// Random graphs whose `indirectbr`s go through `phi`s of `blockaddress`
/// constants (the glue materializes them: `lea r11, [rip + label]`).
#[test]
fn graphs_with_block_address_phis_agree() {
    let o = GenOptions { addr_phis: true, undef: false, ..GenOptions::default() };
    check_graphs(500, 1, 1000, &o, true);
}

/// The code of a random batch keeps the flags rule, and its size
/// against `llc` 22's for the same functions (printed: hod/llc).
#[test]
fn generated_code_keeps_the_flags_rule() {
    let (funcs, _) = batch(7, 600, 1, &GenOptions { undef: false, ..GenOptions::default() });
    let text: String = funcs.iter().map(|f| f.text.as_str()).collect();
    let dir = Scratch::new("x64-control-flags");
    let bytes = object(&text, &default_options()).unwrap_or_else(|e| panic!("{e}"));
    let Some(funcs) = disassemble_bytes(&dir, "hod.o", &bytes) else { return };
    let bad = flags_violations(&funcs);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    let hod_text = text_bytes(&dir.join("hod.o"));
    if let (Some(object), Some(hod)) = (llc(&dir, 22, "llc22", &text), hod_text)
        && let Some(theirs) = text_bytes(&object)
    {
        eprintln!(
            "x64 control: .text hod {hod} llc22 {theirs} bytes: hod/llc {}%",
            hod * 100 / theirs
        );
    }
}

/// The size of `.text` of an object (`llvm-readelf -S`).
fn text_bytes(object: &Path) -> Option<u64> {
    let readelf = tool("llvm-readelf")?;
    let mut command = Command::new(readelf);
    command.arg("-SW").arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    let out = outcome.stdout_text();
    let line = out.lines().find(|l| l.contains(" .text "))?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    let at = fields.iter().position(|&f| f == ".text")?;
    u64::from_str_radix(fields.get(at + 4)?, 16).ok()
}

// ---- switch ----

/// A `switch` function of `bits` over `values` (cases to a few blocks,
/// every fifth to the default), and its calls: every case value, its
/// neighbours, the extremes.
fn switch_func(
    name: &str,
    bits: u32,
    values: &[u64],
    funcs: &mut Vec<Func>,
    calls: &mut Vec<Call>,
) {
    let m = mask(bits);
    let top = 1u64 << (bits - 1);
    let ty = format!("i{bits}");
    let mut text = format!("define i64 @{name}({ty} %x) {{\nentry:\n  switch {ty} %x, label %d [");
    let blocks = values.len().min(9);
    for (i, &v) in values.iter().enumerate() {
        let target = if i % 5 == 4 { "d".to_owned() } else { format!("c{}", i % blocks.max(1)) };
        write!(text, " {ty} {}, label %{target}", signed(bits, v)).unwrap();
    }
    text.push_str(" ]\nd:\n  ret i64 1000\n");
    for c in 0..blocks {
        writeln!(text, "c{c}:\n  ret i64 {c}").unwrap();
    }
    text.push_str("}\n");
    let mut args: BTreeSet<u64> =
        [0, 1, m, m - 1, top, top - 1, top.wrapping_add(1) & m].into_iter().collect();
    for &v in values {
        args.extend([v, v.wrapping_add(1) & m, v.wrapping_sub(1) & m]);
    }
    let f = Func {
        name: name.to_owned(),
        params: vec![Ty::Int(bits)],
        ret: Ty::Int(64),
        text,
        interesting: Vec::new(),
    };
    for a in args {
        calls.push(Call { func: funcs.len(), args: vec![a] });
    }
    funcs.push(f);
}

/// A `switch` per width (`i1` and the odd widths included) and per kind
/// of case list: none, one, few, dense, sparse, negative, the type's
/// extremes, 64-bit values beyond every immediate, a case to the
/// default, several cases to one block, and runs of every length around
/// the search's boundaries (`LINEAR_CASES`, a node and its two halves).
/// Called with every case value, its neighbours and the extremes.
#[test]
fn switch_on_every_width_and_case_list() {
    let mut funcs = Vec::new();
    let mut calls = Vec::new();
    let mut rng = Rng(37);
    let linear = hod_x64::control::LINEAR_CASES;
    for bits in [1u32, 2, 7, 8, 9, 15, 16, 17, 31, 32, 33, 47, 63, 64] {
        let m = mask(bits);
        let top = 1u64 << (bits - 1);
        let room = if bits >= 8 { 64 } else { m as usize };
        let mut lists: Vec<Vec<u64>> = vec![
            vec![],
            vec![m],
            vec![0, m],
            vec![top, top - 1, top.wrapping_add(1) & m, 1],
            (0..room.min(40) as u64).collect(),
            (0..room.min(40) as u64).map(|i| (top.wrapping_sub(20).wrapping_add(i)) & m).collect(),
            (0..room.min(33)).map(|_| rng.next() & m).collect(),
            (0..room.min(12) as u64).map(|i| m.wrapping_sub(i * 3) & m).collect(),
            // Immediate boundaries: 8-bit, 32-bit sign and zero
            // extension, beyond 32 bits.
            [0x7f, 0x80, 0xff, 0x100, 0x7fff_ffff, 0x8000_0000, 0xffff_ffff, 0x1_0000_0000]
                .iter()
                .chain(&[0xffff_ffff_8000_0000, 0xffff_ffff_7fff_ffff, 0x8000_0000_0000_0000])
                .map(|v| v & m)
                .collect(),
        ];
        // The search's shape at its boundaries.
        for len in [linear, linear + 1, 2 * linear + 1, 2 * linear + 2, 4 * linear + 3] {
            lists.push((0..len.min(room) as u64).map(|i| (i * 7 + 3) & m).collect());
        }
        for (k, mut values) in lists.into_iter().enumerate() {
            values.sort_unstable();
            values.dedup();
            if bits < 8 && values.len() as u64 > m {
                values.truncate(m as usize);
            }
            // Shuffled, so that the lowering sorts.
            for i in (1..values.len()).rev() {
                values.swap(i, rng.below(i + 1));
            }
            switch_func(&format!("sw{bits}_{k}"), bits, &values, &mut funcs, &mut calls);
        }
    }
    let Some(wrong) = check("x64-switch", &funcs, &calls, &modes(), true, 37) else { return };
    assert_agree(&wrong, &funcs);
}

/// Cases to the default and several cases to one block (duplicate values
/// are invalid IR the reader rejects), a constant condition, cases all
/// to the default, negative `i64`s beyond the 32-bit immediates.
#[test]
fn switch_corner_cases() {
    let text = r"
define i64 @dup(i32 %x) {
entry:
  switch i32 %x, label %d [ i32 5, label %a i32 -1, label %b i32 7, label %d i32 8, label %a i32 9, label %a ]
a:
  ret i64 1
b:
  ret i64 2
d:
  ret i64 3
}
define i64 @konst(i32 %x) {
entry:
  switch i32 -3, label %d [ i32 5, label %a i32 -3, label %b ]
a:
  ret i64 1
b:
  ret i64 2
d:
  ret i64 3
}
define i64 @alldefault(i64 %x) {
entry:
  switch i64 %x, label %d [ i64 5, label %d i64 -3, label %d ]
d:
  ret i64 3
}
define i64 @wide64(i64 %x) {
entry:
  switch i64 %x, label %d [ i64 -9223372036854775808, label %a i64 -2147483649, label %b i64 4294967296, label %c i64 -2147483648, label %e ]
a:
  ret i64 1
b:
  ret i64 2
c:
  ret i64 4
e:
  ret i64 5
d:
  ret i64 3
}
";
    let one = |name: &str, bits: u32| Func {
        name: name.into(),
        params: vec![Ty::Int(bits)],
        ret: Ty::Int(64),
        text: String::new(),
        interesting: Vec::new(),
    };
    let mut funcs =
        vec![one("dup", 32), one("konst", 32), one("alldefault", 64), one("wide64", 64)];
    funcs[0].text = text.to_owned();
    let mut calls = Vec::new();
    for v in [5u64, 7, 8, 9, 0xffff_ffff, 0, 6] {
        calls.push(Call { func: 0, args: vec![v] });
        calls.push(Call { func: 1, args: vec![v] });
    }
    for v in [5u64, (-3i64) as u64, 0] {
        calls.push(Call { func: 2, args: vec![v] });
    }
    for v in [
        1u64 << 63,
        (-2_147_483_649i64) as u64,
        1 << 32,
        (-2_147_483_648i64) as u64,
        0x8000_0000,
        0xffff_ffff,
        0,
        (1 << 63) + 1,
    ] {
        calls.push(Call { func: 3, args: vec![v] });
    }
    let Some(wrong) = check("x64-switch-corner", &funcs, &calls, &modes(), true, 3) else { return };
    assert_agree(&wrong, &funcs);
}

/// A `switch` of 5000 sparse cases: compiles in time linear-ish in the
/// cases, has no relocation (no jump table), and every case and some
/// misses land right.
#[test]
fn a_switch_of_thousands_of_cases() {
    let mut rng = Rng(5000);
    let mut seen = BTreeSet::new();
    let mut values: Vec<u64> = Vec::new();
    while values.len() < 5000 {
        let v = rng.next() & 0xffff_ffff;
        if seen.insert(v) {
            values.push(v);
        }
    }
    let mut text = String::from("define i64 @big(i32 %x) {\nentry:\n  switch i32 %x, label %d [");
    for (i, &v) in values.iter().enumerate() {
        write!(text, " i32 {}, label %c{}", signed(32, v), i % 64).unwrap();
    }
    text.push_str(" ]\nd:\n  ret i64 99\n");
    for c in 0..64 {
        writeln!(text, "c{c}:\n  ret i64 {c}").unwrap();
    }
    text.push_str("}\n");
    let start = Instant::now();
    let chunk = chunk(&text, &default_options()).unwrap();
    eprintln!("x64 control: 5000 cases compiled in {:?}", start.elapsed());
    assert!(chunk.relocs.is_empty() && chunk.islands.is_empty());
    let f = Func {
        name: "big".into(),
        params: vec![Ty::Int(32)],
        ret: Ty::Int(64),
        text,
        interesting: Vec::new(),
    };
    let mut calls: Vec<Call> =
        values.iter().step_by(7).map(|&v| Call { func: 0, args: vec![v] }).collect();
    for _ in 0..200 {
        calls.push(Call { func: 0, args: vec![rng.next() & 0xffff_ffff] });
    }
    let funcs = [f];
    let Some(wrong) = check("x64-bigswitch", &funcs, &calls, &modes()[..1], true, 5) else {
        return;
    };
    assert_agree(&wrong, &funcs);
}

/// Spells `value` (the low `bits` bits) as a signed decimal.
fn signed_wide(bits: u32, value: u128) -> i128 {
    let shift = 128 - bits;
    ((value << shift) as i128) >> shift
}

/// `switch` on integers of class `Mem` (`i65`, `i100`, `i128`): each
/// function of one `unsigned __int128` argument from a C driver (with
/// junk above the width), against the interpreter and both `llc`s.
#[test]
fn wide_switches_agree() {
    let mut rng = Rng(128);
    let mut functions = String::new();
    let mut expected_calls: Vec<(String, u32, u128)> = Vec::new();
    for bits in [65u32, 100, 128] {
        let m: u128 = if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 };
        let top = 1u128 << (bits - 1);
        let mut random = || (u128::from(rng.next()) << 64) | u128::from(rng.next());
        let mut values = vec![0, 1, u128::from(u64::MAX), 1u128 << 64, (1u128 << 64) + 1, top, m];
        values.push(m - 1);
        for _ in 0..6 {
            values.push(random() & m);
        }
        values.sort_unstable();
        values.dedup();
        let name = format!("w{bits}");
        let ty = format!("i{bits}");
        let mut text =
            format!("define i64 @{name}({ty} %x) {{\nentry:\n  switch {ty} %x, label %d [");
        for (i, &v) in values.iter().enumerate() {
            let target = if i % 5 == 4 { "d".to_owned() } else { format!("c{}", i % 4) };
            write!(text, " {ty} {}, label %{target}", signed_wide(bits, v)).unwrap();
        }
        text.push_str(" ]\nd:\n  ret i64 1000\n");
        for c in 0..4 {
            writeln!(text, "c{c}:\n  ret i64 {c}").unwrap();
        }
        text.push_str("}\n");
        functions.push_str(&text);
        let mut args: BTreeSet<u128> = [0, 1, m, top, top - 1].into_iter().collect();
        for &v in &values {
            args.extend([v, v.wrapping_add(1) & m, v.wrapping_sub(1) & m, v ^ (1 << 64) & m]);
        }
        for a in args {
            expected_calls.push((name.clone(), bits, a));
        }
    }
    // The interpreter's answers.
    let (module, bodies) = parse(&functions);
    let program = Program::with_bodies(&module, &bodies).unwrap();
    let want: Vec<String> = expected_calls
        .iter()
        .map(|(name, bits, a)| {
            let sym = module.find_symbol(name.as_bytes()).unwrap();
            match Interp::new(&program).run(sym, &[Val::int(*bits, *a)]).result {
                Ok(Some(Val::Int { value, .. })) => format!("{}", value as u64),
                other => panic!("{name}: {other:?}"),
            }
        })
        .collect();
    let mut c =
        String::from("#include <stdio.h>\n#include <stdint.h>\ntypedef unsigned __int128 u128;\n");
    for bits in [65, 100, 128] {
        writeln!(c, "int64_t w{bits}(u128);").unwrap();
    }
    c.push_str("int main(void) {\n");
    let mut junk = Rng(9);
    for (name, bits, a) in &expected_calls {
        let a = if *bits < 128 { a | (u128::from(junk.next()) << bits) } else { *a };
        writeln!(
            c,
            "  printf(\"%lld\\n\", (long long){name}(((u128){}ULL << 64) | {}ULL));",
            (a >> 64) as u64,
            a as u64
        )
        .unwrap();
    }
    c.push_str("  return 0;\n}\n");
    let want: Vec<String> =
        want.iter().map(|w| format!("{}", w.parse::<u64>().unwrap() as i64)).collect();
    run_against(&functions, &c, "x64-wide-switch", &want);
}

/// Links `functions` (compiled by hod in every mode and by both `llc`s)
/// with the C `driver`, runs each program once and compares its output
/// lines with `want`.
fn run_against(functions: &str, driver: &str, label: &str, want: &[String]) {
    let dir = Scratch::new(label);
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let Some(linker) = toolchain.linkers.first() else { return };
    let driver = toolchain.compile_c(&dir, "driver", driver, &[]);
    let mut objects = Vec::new();
    for mode in &modes() {
        let bytes = object(functions, &mode.options).unwrap_or_else(|e| panic!("{e}"));
        objects.push((mode.name, dir.write(&format!("{}.o", mode.name), bytes)));
    }
    for (major, name) in graphs::LLC_MAJORS.into_iter().zip(["llc19", "llc22"]) {
        if let Some(object) = llc(&dir, major, name, functions) {
            objects.push((name, object));
        }
    }
    for (name, object) in objects {
        let exe = dir.join(name);
        let ran = toolchain.link_and_run(linker, &[], &[&driver, &object], &exe);
        assert_eq!(exit_code(&ran), Some(0), "{name}: {}", ran.stderr_text());
        let out = ran.stdout_text();
        let got: Vec<&str> = out.lines().collect();
        assert_eq!(got.len(), want.len(), "{name}");
        let wrong: Vec<String> = got
            .iter()
            .zip(want)
            .enumerate()
            .filter(|(_, (g, w))| *g != w)
            .map(|(i, (g, w))| format!("line {i}: want {w} got {g}"))
            .take(8)
            .collect();
        assert!(wrong.is_empty(), "{name}:\n{}", wrong.join("\n"));
    }
}

// ---- fused compares ----

const PREDS: [&str; 10] = ["eq", "ne", "ugt", "uge", "ult", "ule", "sgt", "sge", "slt", "sle"];

/// The constants a compare of `bits` is tried with: every immediate
/// encoding's boundaries, masked to the width.
fn compare_constants(bits: u32) -> Vec<u64> {
    let m = mask(bits);
    let top = 1u64 << (bits - 1);
    let mut values: Vec<u64> = [
        0,
        1,
        m,
        0x7f,
        0x80,
        0xff,
        0x100,
        0x7fff,
        0x8000,
        0xffff,
        0x7fff_ffff,
        0x8000_0000,
        0xffff_ffff,
        0x1_0000_0000,
        0xffff_ffff_8000_0000,
        top,
        top.wrapping_sub(1),
    ]
    .into_iter()
    .map(|v| v & m)
    .collect();
    values.sort_unstable();
    values.dedup();
    values
}

/// The spelling of operand `v` of type `ty` in `.ll`.
fn spell(ty: Ty, v: u64) -> String {
    match ty {
        Ty::Ptr if v == 0 => "null".to_owned(),
        Ty::Ptr => format!("inttoptr (i64 {} to ptr)", v as i64),
        Ty::Int(1) => if v & 1 == 1 { "true" } else { "false" }.to_owned(),
        Ty::Int(bits) => signed(bits, v).to_string(),
    }
}

/// One fused compare: `icmp pred ty lhs, rhs` and a `condbr` on it whose
/// successors both have `phi`s (a constant 0 and an argument incoming,
/// written before the compare, §4.5), returning `p` on the true path
/// and the constant 0 or `q` on the false one. `true_next` lays the
/// true block out next.
fn fused_func(name: &str, ty: Ty, pred: &str, lhs: &str, rhs: &str, true_next: bool) -> Func {
    let t = ty.ll();
    let (first, second) = if true_next {
        (
            "t:\n  %vt = phi i64 [ %p, %entry ]\n  %zt = phi i64 [ 0, %entry ]\n  ret i64 %vt\n",
            "f:\n  %vf = phi i64 [ %q, %entry ]\n  %zf = phi i64 [ 0, %entry ]\n  ret i64 %vf\n",
        )
    } else {
        (
            "f:\n  %vf = phi i64 [ 0, %entry ]\n  %wf = phi i64 [ %q, %entry ]\n  ret i64 %vf\n",
            "t:\n  %vt = phi i64 [ %p, %entry ]\n  %wt = phi i64 [ 0, %entry ]\n  ret i64 %vt\n",
        )
    };
    let text = format!(
        "define i64 @{name}({t} %a, {t} %b, i64 %p, i64 %q) {{\nentry:\n  %c = icmp {pred} {t} {lhs}, {rhs}\n  br i1 %c, label %t, label %f\n{first}{second}}}\n"
    );
    Func {
        name: name.to_owned(),
        params: vec![ty, ty, Ty::Int(64), Ty::Int(64)],
        ret: Ty::Int(64),
        text,
        interesting: Vec::new(),
    }
}

/// The fused-compare functions: every predicate × width (odd ones,
/// pointers) × operand shape (two registers, an operand twice, a
/// constant at every immediate boundary on the right, a few on the
/// left) × layout, with their calls.
fn fused_batch() -> (Vec<Func>, Vec<Call>) {
    let mut funcs = Vec::new();
    let mut calls = Vec::new();
    let mut rng = Rng(124);
    let types = [1u32, 7, 8, 16, 17, 32, 33, 63, 64].map(Ty::Int).into_iter().chain([Ty::Ptr]);
    for ty in types {
        let bits = ty.bits();
        let m = mask(bits);
        let consts = compare_constants(bits);
        for pred in PREDS {
            let mut shapes: Vec<(String, String, Vec<u64>)> = vec![
                ("%a".into(), "%b".into(), Vec::new()),
                ("%a".into(), "%a".into(), Vec::new()),
            ];
            for &k in &consts {
                shapes.push(("%a".into(), spell(ty, k), vec![k]));
            }
            for &k in consts.iter().step_by(4) {
                shapes.push((spell(ty, k), "%a".into(), vec![k]));
            }
            for (i, (lhs, rhs, near)) in shapes.into_iter().enumerate() {
                let name = format!("fc_{}_{pred}_{i}", ty.ll());
                let f = fused_func(&name, ty, pred, &lhs, &rhs, i % 2 == 0);
                let index = funcs.len();
                funcs.push(f);
                let mut picks: Vec<u64> = vec![0, m, 1u64 << (bits - 1)];
                for &k in &near {
                    picks.extend([k, k.wrapping_add(1) & m, k.wrapping_sub(1) & m]);
                }
                picks.push(rng.next() & m);
                for (j, &a) in picks.iter().enumerate() {
                    let b = if j % 2 == 0 { a } else { picks[(j * 7 + 1) % picks.len()] };
                    let p = 0x5eed_0000 + index as u64;
                    calls.push(Call { func: index, args: vec![a, b, p, rng.next() | 1] });
                }
            }
        }
    }
    (funcs, calls)
}

/// Whether `icmp` is deferred on this branch: `int::icmp` is WP-122's,
/// and until it lands it refuses every `icmp`, so the fused tests skip.
fn fused_compares_compile() -> bool {
    let probe = fused_func("probe", Ty::Int(32), "slt", "%a", "%b", true);
    match compile_ll(&probe.text) {
        Ok(()) => true,
        Err(CgError::Unsupported { construct: "x64:todo:int" }) => {
            eprintln!("note: `icmp` is refused (x64:todo:int) before WP-122; skipping");
            false
        }
        Err(e) => panic!("{e}"),
    }
}

/// `condbr` on a fused `icmp`: every predicate × width × operand shape ×
/// layout, the arguments around each constant with junk above their
/// width, against the interpreter and both `llc`s; the successors' `phi`
/// stores (a constant 0 among them, which `xor` would make clobber the
/// flags) do not disturb the branch.
#[test]
fn fused_compares_agree() {
    if !fused_compares_compile() {
        return;
    }
    let (funcs, calls) = fused_batch();
    eprintln!("x64 control: {} fused compares, {} calls", funcs.len(), calls.len());
    let Some(wrong) = check("x64-fused", &funcs, &calls, &modes(), true, 124) else { return };
    assert_agree(&wrong, &funcs);
}

/// Every conditional jump of the fused compares and switches follows
/// its `cmp`/`test` with nothing but flag-keeping glue in between
/// (design 0006 §2.3); in fact with nothing at all.
#[test]
fn fused_compares_keep_the_flags_rule() {
    let mut text = String::new();
    if fused_compares_compile() {
        text.extend(fused_batch().0.into_iter().map(|f| f.text));
    }
    let mut funcs = Vec::new();
    let mut calls = Vec::new();
    for bits in [1u32, 8, 17, 32, 33, 64] {
        let mut values: Vec<u64> = (0..20).map(|i| (i * 0x1_0000_0001) & mask(bits)).collect();
        values.sort_unstable();
        values.dedup();
        switch_func(&format!("s{bits}"), bits, &values, &mut funcs, &mut calls);
    }
    text.extend(funcs.into_iter().map(|f| f.text));
    let dir = Scratch::new("x64-fused-flags");
    let bytes = object(&text, &default_options()).unwrap_or_else(|e| panic!("{e}"));
    let Some(funcs) = disassemble_bytes(&dir, "fused.o", &bytes) else { return };
    let bad = flags_violations(&funcs);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    // Nothing at all between: each `jcc` follows a `cmp`, a `test` or
    // another `jcc`.
    for (name, insts) in &funcs {
        for pair in insts.windows(2) {
            if is_jcc(mnemonic(&pair[1])) {
                let m = mnemonic(&pair[0]);
                assert!(m == "cmp" || m == "test" || is_jcc(m), "{name}: {pair:?}");
            }
        }
    }
}

// ---- indirectbr and blockaddress ----

/// `blockaddress` in data (§9.6): a table of a function's block addresses
/// (one of a block no `indirectbr` lists, bound to the shared trap, a
/// `ud2`), read by the C driver and passed to the function, which jumps
/// through it directly and through a `phi`; a `blockaddress` formed in
/// code (`lea`) and returned, of a listed block and of an unreachable
/// one (the trap again); with and without `-function-sections`, under
/// every linker, as PIE.
#[test]
#[allow(clippy::too_many_lines, reason = "the functions' text and the C driver")]
fn block_address_tables_link_and_run() {
    let functions = r"
$tabbed = comdat any
@tab = global [4 x ptr] [ptr blockaddress(@jump, %one), ptr blockaddress(@jump, %two), ptr blockaddress(@jump, %three), ptr blockaddress(@jump, %dead)]
@vtab = global [3 x ptr] [ptr blockaddress(@via_phi, %one), ptr blockaddress(@via_phi, %two), ptr blockaddress(@via_phi, %three)]
@ctab = linkonce_odr global [2 x ptr] [ptr blockaddress(@tabbed, %x), ptr blockaddress(@tabbed, %y)], comdat($tabbed)

define i64 @jump(ptr %p) {
entry:
  indirectbr ptr %p, [label %one, label %two, label %three]
one:
  ret i64 11
two:
  ret i64 22
three:
  ret i64 33
dead:
  ret i64 44
}

define i64 @via_phi(i1 %c, ptr %p, ptr %q) {
entry:
  br i1 %c, label %a, label %b
a:
  br label %go
b:
  br label %go
go:
  %t = phi ptr [ %p, %a ], [ %q, %b ]
  indirectbr ptr %t, [label %one, label %two, label %three]
one:
  ret i64 1
two:
  ret i64 2
three:
  ret i64 3
}

define linkonce_odr i64 @tabbed(ptr %p) comdat {
entry:
  indirectbr ptr %p, [label %x, label %y]
x:
  ret i64 5
y:
  ret i64 6
}

define ptr @code_addr(i1 %c) {
entry:
  br i1 %c, label %a, label %b
a:
  ret ptr blockaddress(@code_addr, %b)
b:
  ret ptr blockaddress(@code_addr, %dead)
dead:
  ret ptr null
}

define i64 @direct() {
entry:
  indirectbr ptr blockaddress(@direct, %two), [label %one, label %two]
one:
  ret i64 1
two:
  ret i64 2
}
";
    let driver = r#"
#include <stdio.h>
#include <stdint.h>
extern void *tab[4];
extern void *ctab[2];
extern void *vtab[3];
int64_t jump(void *);
int64_t via_phi(_Bool, void *, void *);
int64_t tabbed(void *);
void *code_addr(_Bool);
int64_t direct(void);
static int fails;
#define CHECK(e) do { if (!(e)) { printf("FAIL %s\n", #e); fails++; } } while (0)
static int is_ud2(const void *p) {
    const unsigned char *b = p;
    return b[0] == 0x0f && b[1] == 0x0b;
}
int main(void) {
    CHECK(jump(tab[0]) == 11);
    CHECK(jump(tab[1]) == 22);
    CHECK(jump(tab[2]) == 33);
    CHECK(is_ud2(tab[3]));
    CHECK(via_phi(1, vtab[2], vtab[0]) == 3);
    CHECK(via_phi(0, vtab[2], vtab[0]) == 1);
    CHECK(via_phi(0, vtab[2], vtab[1]) == 2);
    CHECK(tabbed(ctab[0]) == 5);
    CHECK(tabbed(ctab[1]) == 6);
    CHECK(is_ud2(code_addr(0)));
    CHECK(!is_ud2(code_addr(1)) && code_addr(1) != 0);
    CHECK(direct() == 2);
    printf("checked, %d failures\n", fails);
    return fails;
}
"#;
    let dir = Scratch::new("x64-blocktab");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", driver, &[]);
    for sections in [false, true] {
        let options =
            Options { function_sections: sections, data_sections: sections, ..default_options() };
        let bytes = object(functions, &options).unwrap_or_else(|e| panic!("{e}"));
        let object = dir.write(&format!("blocks{sections}.o"), bytes);
        for linker in &toolchain.linkers {
            let program = dir.join(format!("blocks-{sections}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, &object], &program);
            assert_eq!(
                exit_code(&ran),
                Some(0),
                "{}:\n{}{}",
                linker.name,
                ran.stdout_text(),
                ran.stderr_text()
            );
        }
    }
}

// ---- forms and refusals ----

/// The disassembly of the one function of `f`.
fn insts_of(f: &str) -> Option<Vec<String>> {
    let dir = Scratch::new("x64-control-forms");
    let bytes = object(f, &default_options()).unwrap_or_else(|e| panic!("{e}"));
    let funcs = disassemble_bytes(&dir, "f.o", &bytes)?;
    Some(funcs.into_iter().next()?.1)
}

/// The forms the lowering chooses: `test r8, 1` and one `je` to the
/// false block when the true block is next, one `jne` to the true block
/// when the false block is next; no test at all for a constant
/// condition, two equal targets or a constant `switch`; `indirectbr`
/// through a register is `jmp r`.
#[test]
fn the_branch_forms() {
    let jccs = |insts: &[String]| -> Vec<String> {
        insts.iter().filter(|i| is_jcc(mnemonic(i))).map(|i| mnemonic(i).to_owned()).collect()
    };
    let Some(next_true) = insts_of(
        "define i64 @f(i1 %c) {\ne:\n br i1 %c, label %t, label %f\nt:\n ret i64 1\nf:\n ret i64 2\n}\n",
    ) else {
        return;
    };
    assert!(
        next_true.iter().any(|i| i.starts_with("test ") && i.ends_with(", 0x1")),
        "{next_true:?}"
    );
    assert_eq!(jccs(&next_true), ["je"], "{next_true:?}");
    let next_false = insts_of(
        "define i64 @f(i1 %c) {\ne:\n br i1 %c, label %t, label %f\nf:\n ret i64 2\nt:\n ret i64 1\n}\n",
    )
    .unwrap();
    assert_eq!(jccs(&next_false), ["jne"], "{next_false:?}");
    for f in [
        "define i64 @f(i1 %c) {\ne:\n br i1 true, label %t, label %f\nf:\n ret i64 2\nt:\n ret i64 1\n}\n",
        "define i64 @f(i1 %c) {\ne:\n br i1 undef, label %t, label %f\nf:\n ret i64 2\nt:\n ret i64 1\n}\n",
        "define i64 @f(i1 %c) {\ne:\n br i1 %c, label %t, label %t\nt:\n ret i64 1\n}\n",
        "define i64 @f(i8 %c) {\ne:\n switch i8 7, label %t [ i8 7, label %u ]\nt:\n ret i64 1\nu:\n ret i64 3\n}\n",
    ] {
        let insts = insts_of(f).unwrap();
        assert!(
            !insts
                .iter()
                .any(|i| mnemonic(i) == "test" || mnemonic(i) == "cmp" || is_jcc(mnemonic(i))),
            "{f}: {insts:?}"
        );
    }
    let jmp_r =
        insts_of("define i64 @f(ptr %p) {\n indirectbr ptr %p, [label %a]\na:\n ret i64 1\n}\n")
            .unwrap();
    assert!(jmp_r.iter().any(|i| i.starts_with("jmp r")), "{jmp_r:?}");
}

#[test]
fn the_group_is_no_longer_refused() {
    for f in [
        "define i64 @f(i1 %c) {\n br i1 %c, label %a, label %b\na:\n ret i64 1\nb:\n ret i64 2\n}",
        "define i64 @f(i64 %x) {\n switch i64 %x, label %a [ i64 1, label %b ]\na:\n ret i64 1\nb:\n ret i64 2\n}",
        "define i64 @f(i128 %x) {\n switch i128 %x, label %a [ i128 1, label %b ]\na:\n ret i64 1\nb:\n ret i64 2\n}",
        "define i64 @f(ptr %p) {\n indirectbr ptr %p, [label %a]\na:\n ret i64 1\n}",
        "define ptr @f() {\n ret ptr blockaddress(@f, %a)\na:\n ret ptr null\n}",
    ] {
        assert_eq!(compile_ll(f), Ok(()), "{f}");
    }
}

/// A `blockaddress` of another function's block, used in code, is
/// refused by name (`classify` rejects it before code generation).
#[test]
fn a_foreign_block_address_is_refused() {
    let f = "define i64 @g() {\nentry:\n  br label %x\nx:\n  ret i64 1\n}\n\
             define ptr @f() {\n  ret ptr blockaddress(@g, %x)\n}\n";
    assert_eq!(compile_ll(f), Err(CgError::unsupported(hod_x64::control::BLOCKADDRESS)));
}
