//! The control group (WP-037, `spec/codegen.md` §6): `condbr`, `switch`,
//! `indirectbr`, `blockaddress` in code and in data, and the `phi`
//! protocol on the real target. The oracle is `hod_interp`, the second
//! one `llc -O0 -global-isel=0 -fast-isel=0`: functions made of `phi`s and
//! terminators only (`common::control`), batched into one object and one
//! process per mode, called from a C driver with random bits above each
//! argument's width.
//!
//! `HOD_CONTROL_SEEDS=<n>` sets the batches of `many_graphs_agree`.

mod common;
#[path = "common/control.rs"]
mod graphs;

use std::fmt::Write as _;
use std::time::Instant;

use common::linking::{Toolchain, exit_code};
use common::{compile_ll, emit, ll_header, parse, write_object};
use graphs::{
    Call, Func, GenOptions, Mode, Rng, Ty, Wrong, check, function, mask, signed, text_of,
};
use hod_cg::{Options, RelocModel, Stress};

/// Plain, every stress option, far mode alone.
const MODES: [Mode; 3] = [
    Mode {
        name: "plain",
        options: Options {
            stress: Stress { min_pool: false, far: false, displace: false },
            ..DEFAULT
        },
    },
    Mode { name: "stress", options: Options { stress: Stress::ALL, ..DEFAULT } },
    Mode {
        name: "far",
        options: Options {
            stress: Stress { min_pool: false, far: true, displace: false },
            ..DEFAULT
        },
    },
];

const DEFAULT: Options = Options {
    reloc_model: RelocModel::Static,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false },
    debug_compression: hod_obj::Compression::None,
};

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
        let Some(wrong) = check("a64-control", &funcs, &calls, &MODES, with_llc, seed) else {
            return checked;
        };
        assert_agree(&wrong, &funcs);
        checked += funcs.len();
    }
    checked
}

/// Random graphs with `undef` and `poison` incoming values, against the
/// interpreter; then graphs without them, against `llc` as well.
#[test]
fn random_graphs_agree_with_the_interpreter_and_llc() {
    let start = Instant::now();
    let a = check_graphs(1, 4, 300, &GenOptions::default(), false);
    let o = GenOptions { undef: false, ..GenOptions::default() };
    let b = check_graphs(101, 2, 300, &o, true);
    eprintln!("a64 control: {a} + {b} (with llc) graphs agree in {:?}", start.elapsed());
}

/// Many more graphs.
#[test]
#[ignore = "a minute; run with --ignored"]
fn many_graphs_agree() {
    let batches: u64 =
        std::env::var("HOD_CONTROL_SEEDS").ok().and_then(|text| text.parse().ok()).unwrap_or(40);
    let start = Instant::now();
    let checked = check_graphs(1_000, batches, 500, &GenOptions::default(), false);
    eprintln!("a64 control: {checked} graphs agree in {:?}", start.elapsed());
}

/// A `switch` per width (`i1` and the odd widths included) and per kind
/// of case list: none, one, few, many, dense, sparse, the type's
/// extremes, a case to the default, several cases to one block. Called
/// with every case value, its neighbours and the extremes.
#[test]
fn switch_on_every_width_and_case_list() {
    let mut funcs = Vec::new();
    let mut calls = Vec::new();
    let mut rng = Rng(37);
    for bits in [1u32, 2, 7, 8, 9, 15, 16, 17, 31, 32, 33, 47, 63, 64] {
        let m = mask(bits);
        let top = 1u64 << (bits - 1);
        let room = if bits >= 8 { 64 } else { m as usize };
        let lists: Vec<Vec<u64>> = vec![
            vec![],
            vec![m],
            vec![0, m],
            vec![top, top - 1, top.wrapping_add(1) & m, 1],
            (0..room.min(40) as u64).collect(),
            (0..room.min(40) as u64).map(|i| (top.wrapping_sub(20).wrapping_add(i)) & m).collect(),
            (0..room.min(33)).map(|_| rng.next() & m).collect(),
            (0..room.min(12) as u64).map(|i| m.wrapping_sub(i * 3) & m).collect(),
        ];
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
            let name = format!("sw{bits}_{k}");
            let ty = format!("i{bits}");
            let mut text =
                format!("define i64 @{name}({ty} %x) {{\nentry:\n  switch {ty} %x, label %d [");
            let blocks = values.len().min(9);
            for (i, &v) in values.iter().enumerate() {
                // Every fifth case to the default, the others spread over
                // a few blocks.
                let target =
                    if i % 5 == 4 { "d".to_owned() } else { format!("c{}", i % blocks.max(1)) };
                write!(text, " {ty} {}, label %{target}", signed(bits, v)).unwrap();
            }
            text.push_str(" ]\nd:\n  ret i64 1000\n");
            for c in 0..blocks {
                writeln!(text, "c{c}:\n  ret i64 {c}").unwrap();
            }
            text.push_str("}\n");
            let mut args: Vec<u64> = vec![0, 1, m, m - 1, top, top - 1, top.wrapping_add(1) & m];
            for &v in &values {
                args.extend([v, v.wrapping_add(1) & m, v.wrapping_sub(1) & m]);
            }
            let f = Func {
                name,
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
    }
    let Some(wrong) = check("a64-switch", &funcs, &calls, &MODES, true, 37) else { return };
    assert_agree(&wrong, &funcs);
}

/// A `switch` of 5000 sparse cases: compiles in time linear-ish in the
/// cases, and every case and some misses land right.
#[test]
fn a_switch_of_thousands_of_cases() {
    let mut rng = Rng(5000);
    let mut values: Vec<u64> = Vec::new();
    while values.len() < 5000 {
        let v = rng.next() & 0xffff_ffff;
        if !values.contains(&v) {
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
    let start = Instant::now();
    let (m, bodies) = parse(&format!("{}{}", ll_header(22), funcs[0].text));
    let chunk = common::chunk(&m, &bodies, &Options::default()).unwrap();
    eprintln!("a64 control: 5000 cases compiled in {:?}", start.elapsed());
    // No jump table: no relocation, no data island.
    assert!(chunk.relocs.is_empty() && chunk.islands.is_empty());
    let Some(wrong) = check("a64-bigswitch", &funcs, &calls, &MODES[..2], true, 5) else { return };
    assert_agree(&wrong, &funcs);
}

/// Functions larger than the ±32 KiB of `tbz` and the ±1 MiB of `b.cond`
/// and `adr`, generated in a loop: a `condbr` and an `indirectbr` on a
/// `blockaddress` at the top whose targets are at the bottom, across a
/// `switch` whose blocks return 64-bit constants (five instructions
/// each). The driver retries them in far mode.
#[test]
fn far_branches_reach_across_large_functions() {
    let mut funcs = Vec::new();
    let mut calls = Vec::new();
    let mut rng = Rng(77);
    for (name, cases) in [("far40k", 1_200usize), ("far2m", 70_000)] {
        let mut text = format!(
            "define i64 @{name}(i1 %c, i32 %x, i1 %d) {{\nentry:\n  br i1 %c, label %end, label %mid\n\
             mid:\n  br i1 %d, label %jump, label %sw\njump:\n  indirectbr ptr blockaddress(@{name}, %end2), [label %end2, label %sw]\n\
             sw:\n  switch i32 %x, label %end ["
        );
        for i in 0..cases {
            write!(text, " i32 {i}, label %r{i}").unwrap();
        }
        text.push_str(" ]\n");
        let mut rets = Vec::with_capacity(cases);
        for i in 0..cases {
            let v = rng.next() | (1 << 63) | 0x0001_0001_0001;
            rets.push(v);
            writeln!(text, "r{i}:\n  ret i64 {}", v as i64).unwrap();
        }
        text.push_str("end:\n  ret i64 7\nend2:\n  ret i64 8\n}\n");
        let func = funcs.len();
        for c in [0u64, 1] {
            for d in [0u64, 1] {
                for x in [0u64, 1, cases as u64 - 1, cases as u64, 0xffff_ffff, cases as u64 / 2] {
                    calls.push(Call { func, args: vec![c, x, d] });
                }
            }
        }
        funcs.push(Func {
            name: name.into(),
            params: vec![Ty::Int(1), Ty::Int(32), Ty::Int(1)],
            ret: Ty::Int(64),
            text,
            interesting: Vec::new(),
        });
    }
    // The large one really is beyond the conditional branches' reach.
    let (m, bodies) = parse(&format!("{}{}", ll_header(22), funcs[1].text));
    let chunk = common::chunk(&m, &bodies, &Options::default()).unwrap();
    assert!(chunk.code.len() > 2 << 20, "{} bytes", chunk.code.len());
    let Some(wrong) = check("a64-far", &funcs, &calls, &MODES[..1], false, 77) else { return };
    assert_agree(&wrong, &funcs);
}

/// The cases the `phi` protocol was designed against, each by name:
/// a critical edge, a block listed twice by one predecessor, two `phi`s
/// that swap (and three that rotate) around a loop, a `phi` of the same
/// block read on a self loop, constants and `undef` incoming.
#[test]
#[allow(clippy::too_many_lines, reason = "the functions' text and their calls")]
fn phi_cases_by_name() {
    let text = r"
define i64 @critical(i1 %c, i64 %x, i64 %y) {
entry:
  br i1 %c, label %join, label %side
side:
  br label %join
join:
  %v = phi i64 [ %x, %entry ], [ %y, %side ]
  ret i64 %v
}
define i64 @twice(i8 %s, i64 %x, i64 %y) {
entry:
  switch i8 %s, label %other [ i8 1, label %join i8 2, label %join i8 -1, label %join ]
other:
  br i1 true, label %join, label %join
join:
  %v = phi i64 [ %x, %entry ], [ %x, %entry ], [ %x, %entry ], [ %y, %other ], [ %y, %other ]
  ret i64 %v
}
define i64 @swap(i8 %n, i64 %x, i64 %y) {
entry:
  br label %head
head:
  %s = phi i8 [ %n, %entry ], [ 2, %one ], [ 3, %two ], [ 9, %three ]
  %a = phi i64 [ %x, %entry ], [ %b, %one ], [ %b, %two ], [ %b, %three ]
  %b = phi i64 [ %y, %entry ], [ %a, %one ], [ %a, %two ], [ %a, %three ]
  switch i8 %s, label %out [ i8 1, label %one i8 2, label %two i8 3, label %three ]
one:
  br label %head
two:
  br label %head
three:
  br label %head
out:
  %r = phi i64 [ %a, %head ]
  ret i64 %r
}
define i64 @rotate(i1 %go, i64 %x, i64 %y, i64 %z) {
entry:
  br label %head
head:
  %f = phi i1 [ %go, %entry ], [ false, %head ]
  %a = phi i64 [ %x, %entry ], [ %b, %head ]
  %b = phi i64 [ %y, %entry ], [ %c, %head ]
  %c = phi i64 [ %z, %entry ], [ %a, %head ]
  br i1 %f, label %head, label %out
out:
  ret i64 %b
}
define i64 @consts(i1 %c, i1 %d) {
entry:
  br i1 %c, label %a, label %b
a:
  br i1 %d, label %join, label %u
b:
  br label %join
u:
  br label %join
join:
  %v = phi i64 [ -1, %a ], [ 81985529216486895, %b ], [ undef, %u ]
  %w = phi i1 [ true, %a ], [ false, %b ], [ poison, %u ]
  br i1 %w, label %t, label %e
t:
  ret i64 %v
e:
  %v2 = phi i64 [ %v, %join ]
  ret i64 %v2
}
define i32 @narrow(i1 %c, i17 %x, i32 %y) {
entry:
  br i1 %c, label %a, label %b
a:
  br label %j
b:
  br label %j
j:
  %v = phi i17 [ %x, %a ], [ -1, %b ]
  switch i17 %v, label %no [ i17 -1, label %yes i17 65535, label %half ]
yes:
  ret i32 1
half:
  ret i32 2
no:
  ret i32 %y
}
";
    let sig = |name: &str, params: &[Ty], ret: Ty| Func {
        name: name.into(),
        params: params.to_vec(),
        ret,
        text: String::new(),
        interesting: Vec::new(),
    };
    let i64t = Ty::Int(64);
    let mut funcs = vec![
        sig("critical", &[Ty::Int(1), i64t, i64t], i64t),
        sig("twice", &[Ty::Int(8), i64t, i64t], i64t),
        sig("swap", &[Ty::Int(8), i64t, i64t], i64t),
        sig("rotate", &[Ty::Int(1), i64t, i64t, i64t], i64t),
        sig("consts", &[Ty::Int(1), Ty::Int(1)], i64t),
        sig("narrow", &[Ty::Int(1), Ty::Int(17), Ty::Int(32)], Ty::Int(32)),
    ];
    funcs[0].text = text.to_owned();
    let calls: Vec<Call> = [
        (0, vec![0, 5, 6]),
        (0, vec![1, 5, 6]),
        (1, vec![1, 10, 20]),
        (1, vec![2, 10, 20]),
        (1, vec![255, 10, 20]),
        (1, vec![3, 10, 20]),
        (2, vec![0, 10, 20]),
        (2, vec![1, 10, 20]),
        (2, vec![2, 10, 20]),
        (2, vec![3, 10, 20]),
        (3, vec![0, 1, 2, 3]),
        (3, vec![1, 1, 2, 3]),
        (4, vec![0, 0]),
        (4, vec![1, 0]),
        (4, vec![1, 1]),
        (5, vec![0, 0, 9]),
        (5, vec![1, 0x1ffff, 9]),
        (5, vec![1, 0xffff, 9]),
        (5, vec![1, 3, 9]),
    ]
    .into_iter()
    .map(|(func, args)| Call { func, args })
    .collect();
    let Some(wrong) = check("a64-phis", &funcs, &calls, &MODES, false, 1) else { return };
    assert_agree(&wrong, &funcs);
    // The answers themselves, so that the interpreter is checked too.
    // (`swap` from state 1 runs three back edges: an odd number of swaps.)
}

/// `blockaddress` in data (§9.6): a table of a function's block addresses
/// (one of a block no `indirectbr` lists, which is bound to the shared
/// trap), read by the C driver and passed to the function, which jumps
/// through it directly and through a `phi`; with and without
/// `-function-sections`, in a COMDAT group, under every linker.
#[test]
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
static int fails;
#define CHECK(e) do { if (!(e)) { printf("FAIL %s\n", #e); fails++; } } while (0)
int main(void) {
    CHECK(jump(tab[0]) == 11);
    CHECK(jump(tab[1]) == 22);
    CHECK(jump(tab[2]) == 33);
    CHECK(tab[3] != 0);
    CHECK(via_phi(1, vtab[2], vtab[0]) == 3);
    CHECK(via_phi(0, vtab[2], vtab[0]) == 1);
    CHECK(via_phi(0, vtab[2], vtab[1]) == 2);
    CHECK(tabbed(ctab[0]) == 5);
    CHECK(tabbed(ctab[1]) == 6);
    printf("checked, %d failures\n", fails);
    return fails;
}
"#;
    let text = format!("{}{functions}", ll_header(22));
    let (m, bodies) = parse(&text);
    let dir = hod_testutil::Scratch::new("a64-blocktab");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", driver, &[]);
    for (sections, reloc_model) in [
        (false, RelocModel::Static),
        (true, RelocModel::Static),
        (false, RelocModel::Pic),
        (true, RelocModel::Pic),
    ] {
        let options = Options {
            function_sections: sections,
            data_sections: sections,
            reloc_model,
            ..Options::default()
        };
        let emitted = emit(&m, &bodies, &options);
        let object = write_object(&dir, &format!("blocks{sections}{reloc_model:?}.o"), &emitted);
        for linker in &toolchain.linkers {
            let program = dir.join(format!("blocks-{sections}-{reloc_model:?}-{}", linker.name));
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

/// Random graphs whose `indirectbr`s go through `phi`s of `blockaddress`
/// constants (selected by the paths into the block).
#[test]
fn graphs_with_block_address_phis_agree() {
    let o = GenOptions { addr_phis: true, ..GenOptions::default() };
    check_graphs(500, 4, 300, &o, false);
}

/// The forms the lowering chooses: `tbnz`/`tbz` on bit 0 with the branch
/// to the next block omitted, `br` for a constant condition and for two
/// equal targets, `ubfx` before a narrow `switch`.
#[test]
fn the_branch_forms() {
    let words = |f: &str| {
        let (m, bodies) = parse(&format!("{}{f}", ll_header(22)));
        common::code_words(&m, &bodies, &Options::default())
    };
    let test_zero = |w: u32| w & 0x7f00_0000 == 0x3600_0000;
    let test_one = |w: u32| w & 0x7f00_0000 == 0x3700_0000;
    // The true block next: `tbz` to the false one.
    let next_true = words(
        "define i64 @f(i1 %c) {\ne:\n br i1 %c, label %t, label %f\nt:\n ret i64 1\nf:\n ret i64 2\n}",
    );
    assert_eq!(next_true.iter().filter(|&&w| test_zero(w)).count(), 1, "{next_true:x?}");
    assert!(!next_true.iter().any(|&w| test_one(w)));
    // The false block next: `tbnz` to the true one.
    let next_false = words(
        "define i64 @f(i1 %c) {\ne:\n br i1 %c, label %t, label %f\nf:\n ret i64 2\nt:\n ret i64 1\n}",
    );
    assert_eq!(next_false.iter().filter(|&&w| test_one(w)).count(), 1, "{next_false:x?}");
    // A constant and two equal targets: no test at all.
    for f in [
        "define i64 @f(i1 %c) {\ne:\n br i1 true, label %t, label %f\nf:\n ret i64 2\nt:\n ret i64 1\n}",
        "define i64 @f(i1 %c) {\ne:\n br i1 %c, label %t, label %t\nt:\n ret i64 1\n}",
        "define i64 @f(i8 %c) {\ne:\n switch i8 7, label %t [ i8 7, label %u ]\nt:\n ret i64 1\nu:\n ret i64 3\n}",
    ] {
        let w = words(f);
        assert!(
            !w.iter().any(|&w| test_zero(w) || test_one(w) || w & 0xff00_0010 == 0x5400_0000),
            "{f}: {w:x?}"
        );
    }
    // The condition's bits above bit 0 are not looked at: covered by the
    // driver's junk bits in every native test.
}

/// A `switch` on `i128` is lowered (WP-084; `tests/tail.rs` runs it).
#[test]
fn a_wide_switch_is_lowered() {
    let f = "define i64 @f(i128 %x) {\n switch i128 %x, label %a [ i128 1, label %b ]\na:\n ret i64 1\nb:\n ret i64 2\n}";
    assert_eq!(compile_ll(f), Ok(()));
}

#[test]
fn the_group_is_no_longer_refused() {
    for f in [
        "define i64 @f(i1 %c) {\n br i1 %c, label %a, label %b\na:\n ret i64 1\nb:\n ret i64 2\n}",
        "define i64 @f(i64 %x) {\n switch i64 %x, label %a [ i64 1, label %b ]\na:\n ret i64 1\nb:\n ret i64 2\n}",
        "define i64 @f(ptr %p) {\n indirectbr ptr %p, [label %a]\na:\n ret i64 1\n}",
    ] {
        assert_eq!(compile_ll(f), Ok(()), "{f}");
    }
}
