//! The exclusive-access intrinsics (WP-245, `spec/codegen.md` §5.6.39):
//! `llvm.aarch64.ldxr`, `ldaxr`, `stxr`, `stlxr` at each size, the pair
//! forms and `clrex`.
//!
//! - The instruction hod emits for each intrinsic is the one `llc -O0
//!   -global-isel=0 -fast-isel=0` of LLVM 19 and 22 emits, word for word
//!   with the register fields masked out.
//! - A table of functions runs natively, built by hod plain, stressed and
//!   far and by `llc`, under every linker (and once more under
//!   `qemu-aarch64 -cpu cortex-a53` when it is installed on an AArch64
//!   host), against expectations computed here: retry loops at each
//!   size, the zero-extension of what is loaded, the low bits a store
//!   writes, pairs in order, and `clrex` followed by a store-exclusive,
//!   whose status must be 1. Every target is a global that fills a
//!   2048-byte granule, away from the stack.
//! - Retry loops whose target is in their own frame, bounded, stop and
//!   leave memory as their status says; that they ever succeed is not
//!   asserted (on hardware a frame store between the pair may hit the
//!   target's reservation granule, §5.6.39).
//! - Four threads count through an `ldxr`/`stxr` retry loop of one block
//!   at each size and through `ldaxp`/`stlxp`; every total is exact.
//! - What hod cannot read is refused by name.
//!
//! Emulation: `qemu-user` implements `clrex` by forgetting the reserved
//! address, so the forced failure holds there as on hardware (where the
//! architecture guarantees it). No assertion depends on a store-exclusive
//! succeeding at its first attempt: every other store is in a retry loop,
//! which the architecture allows to fail spuriously and qemu does not.

mod common;
#[path = "common/native.rs"]
mod native;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::Toolchain;
use common::{assert_refused, chunk, ll_header, llc, parse, write_object};
use hod_a64::compile_module;
use hod_a64::exclusive::{ELEMENT, TYPE};
use hod_cg::{Options, RelocModel, Stress};
use hod_testutil::{LLVM_MAJORS, Scratch, find_tool, run_with, tool};

const DEFAULT: Options = Options {
    reloc_model: RelocModel::Static,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false, coro_frame: false },
    debug_compression: hod_obj::Compression::None,
    split_dwarf: hod_dwarf::SplitDwarf::Off,
    tls_descriptors: false,
};

const MODES: [(&str, Options); 3] = [
    ("plain", DEFAULT),
    ("stress", Options { stress: Stress::ALL, ..DEFAULT }),
    (
        "far",
        Options {
            stress: Stress { min_pool: false, far: true, displace: false, coro_frame: false },
            ..DEFAULT
        },
    ),
];

const DECLARE: &str = "declare i64 @llvm.aarch64.ldxr.p0(ptr)\n\
     declare i64 @llvm.aarch64.ldaxr.p0(ptr)\n\
     declare i32 @llvm.aarch64.stxr.p0(i64, ptr)\n\
     declare i32 @llvm.aarch64.stlxr.p0(i64, ptr)\n\
     declare { i64, i64 } @llvm.aarch64.ldxp(ptr)\n\
     declare { i64, i64 } @llvm.aarch64.ldaxp(ptr)\n\
     declare i32 @llvm.aarch64.stxp(i64, i64, ptr)\n\
     declare i32 @llvm.aarch64.stlxp(i64, i64, ptr)\n\
     declare void @llvm.aarch64.clrex()\n";

/// The access sizes, in bits.
const BITS: [u32; 4] = [8, 16, 32, 64];

fn mask(bits: u32) -> u64 {
    if bits == 64 { u64::MAX } else { (1 << bits) - 1 }
}

/// One function per intrinsic and size, each its intrinsic alone.
fn single_body() -> (String, usize) {
    let mut s = String::from(DECLARE);
    let mut count = 0;
    for bits in BITS {
        for name in ["ldxr", "ldaxr"] {
            writeln!(
                s,
                "define i64 @{name}{bits}(ptr %p) {{\n  %v = call i64 @llvm.aarch64.{name}.p0(ptr \
                 elementtype(i{bits}) %p)\n  ret i64 %v\n}}"
            )
            .unwrap();
            count += 1;
        }
        for name in ["stxr", "stlxr"] {
            writeln!(
                s,
                "define i32 @{name}{bits}(i64 %v, ptr %p) {{\n  %s = call i32 \
                 @llvm.aarch64.{name}.p0(i64 %v, ptr elementtype(i{bits}) %p)\n  ret i32 %s\n}}"
            )
            .unwrap();
            count += 1;
        }
    }
    for name in ["ldxp", "ldaxp"] {
        writeln!(
            s,
            "define {{ i64, i64 }} @{name}(ptr %p) {{\n  %v = call {{ i64, i64 }} \
             @llvm.aarch64.{name}(ptr %p)\n  ret {{ i64, i64 }} %v\n}}"
        )
        .unwrap();
        count += 1;
    }
    for name in ["stxp", "stlxp"] {
        writeln!(
            s,
            "define i32 @{name}(i64 %a, i64 %b, ptr %p) {{\n  %s = call i32 \
             @llvm.aarch64.{name}(i64 %a, i64 %b, ptr %p)\n  ret i32 %s\n}}"
        )
        .unwrap();
        count += 1;
    }
    s.push_str("define void @clrex() {\n  call void @llvm.aarch64.clrex()\n  ret void\n}\n");
    (s, count + 1)
}

const CLREX: u32 = 0xd503_3f5f;

/// The exclusive-access words of `code`, their register fields masked:
/// `Rt` and `Rn` always, `Rs` of a store, `Rt2` of a pair; and `clrex`.
fn exclusive(code: &[u32]) -> Vec<u32> {
    code.iter()
        .filter(|&&word| word == CLREX || word & 0x3f80_0000 == 0x0800_0000)
        .map(|&word| {
            if word == CLREX {
                return word;
            }
            let mut m = !0x3ff_u32;
            if word & (1 << 22) == 0 {
                m &= !(0x1f << 16);
            }
            if word & (1 << 21) != 0 {
                m &= !(0x1f << 10);
            }
            word & m
        })
        .collect()
}

/// The code words of an object's `.text`, through `llvm-objdump`.
fn object_words(object: &Path) -> Option<Vec<u32>> {
    let objdump = tool("llvm-objdump")?;
    let mut command = Command::new(objdump);
    command.args(["-d", "--no-leading-addr"]).arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    Some(
        outcome
            .stdout_text()
            .lines()
            .filter(|line| line.starts_with(char::is_whitespace))
            .filter_map(|line| {
                let hex = line.split_whitespace().next()?;
                if hex.len() == 8 { u32::from_str_radix(hex, 16).ok() } else { None }
            })
            .collect(),
    )
}

/// `llc` of `major` on `text`, to an object; `None` if not installed.
fn llc_of(dir: &Scratch, major: u32, text: &str) -> Option<PathBuf> {
    let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if !llc.is_file() {
        return None;
    }
    let input = dir.write(&format!("llc-{major}.ll"), text);
    let output = dir.join(format!("llc-{major}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llc-{major}: {}", outcome.stderr_text());
    Some(output)
}

// §5.6.39: each intrinsic at each size is the instruction `llc` 19 and
// 22 select, in every mode.
#[test]
fn instructions_match_llc() {
    let (body, count) = single_body();
    let (module, bodies) = parse(&format!("{}{body}", ll_header(22)));
    let mut ours = None;
    for (mode, options) in &MODES {
        let code = chunk(&module, &bodies, options).unwrap_or_else(|e| panic!("{mode}: {e}"));
        let words: Vec<u32> =
            code.code.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let words = exclusive(&words);
        assert_eq!(words.len(), count, "{mode}: one exclusive instruction per function");
        match &ours {
            None => ours = Some(words),
            Some(first) => assert_eq!(&words, first, "{mode}"),
        }
    }
    let ours = ours.expect("a mode");
    let dir = Scratch::new("a64-exclusive-enc");
    let mut checked = 0;
    for major in LLVM_MAJORS {
        let Some(object) = llc_of(&dir, major, &format!("{}{body}", ll_header(major))) else {
            continue;
        };
        let Some(theirs) = object_words(&object) else { continue };
        assert_eq!(exclusive(&theirs), ours, "llc-{major}");
        checked += 1;
    }
    if hod_testutil::tools_required() {
        assert_eq!(checked, LLVM_MAJORS.len(), "every llc version");
    }
    eprintln!("{count} exclusive instructions agree with {checked} llc versions");
}

/// The functions of the native table, and the calls with their expected
/// lines.
/// Where the table's functions keep their targets: a global that fills
/// a granule of the largest size the architecture allows (2048 bytes),
/// so that no store of the function's own frame, which is on the stack,
/// can clear the exclusive monitor (§5.6.39, "The reservation granule").
const CELL: &str = "@cell = internal global [256 x i64] zeroinitializer, align 2048\n";

/// The first instruction of each table function: `%p` is the cell.
const AT_CELL: &str = "%p = getelementptr i8, ptr @cell, i64 0";

fn table() -> (String, Vec<native::Call>, String) {
    let mut s = format!("{DECLARE}{CELL}");
    let mut calls: Vec<native::Call> = Vec::new();
    let mut want = String::new();
    let xs = [0u64, 0x80, 0xfedc_ba98_7654_3210, u64::MAX, 0x8000_0000_8000_8080];
    for bits in BITS {
        let m = mask(bits);
        for (load, store) in [("ldxr", "stxr"), ("ldaxr", "stlxr")] {
            // What is loaded is zero-extended.
            writeln!(
                s,
                "define i64 @{load}{bits}(i64 %x) {{\n  {AT_CELL}\n  \
                 store i64 %x, ptr %p\n  %v = call i64 @llvm.aarch64.{load}.p0(ptr \
                 elementtype(i{bits}) %p)\n  call void @llvm.aarch64.clrex()\n  ret i64 %v\n}}"
            )
            .unwrap();
            // A store writes the low bits of its operand, nothing else.
            writeln!(
                s,
                "define i64 @{store}{bits}(i64 %x, i64 %v) {{\n  {AT_CELL}\n  \
                 store i64 %x, ptr %p\n  br label %again\nagain:\n  %o = call i64 \
                 @llvm.aarch64.{load}.p0(ptr elementtype(i{bits}) %p)\n  %s = call i32 \
                 @llvm.aarch64.{store}.p0(i64 %v, ptr elementtype(i{bits}) %p)\n  \
                 %ok = icmp eq i32 %s, 0\n  br i1 %ok, label %done, label %again\ndone:\n  \
                 %r = load i64, ptr %p\n  ret i64 %r\n}}"
            )
            .unwrap();
            for &x in &xs {
                calls.push((format!("{load}{bits}"), [x, 0, 0, 0, 0, 0]));
                writeln!(want, "{}", x & m).unwrap();
                let v = x.rotate_left(13) ^ 0x5a5a_5a5a_5a5a_5a5a;
                calls.push((format!("{store}{bits}"), [!x, v, 0, 0, 0, 0]));
                writeln!(want, "{}", (!x & !m) | (v & m)).unwrap();
            }
        }
        // A retry loop that increments `k` times, the loop in one block.
        writeln!(
            s,
            "define i64 @inc{bits}(i64 %x, i64 %k) {{\nentry:\n  {AT_CELL}\n  \
             store i64 %x, ptr %p\n  br label %outer\nouter:\n  %i = phi i64 [ 0, %entry ], \
             [ %i1, %next ]\n  %end = icmp eq i64 %i, %k\n  br i1 %end, label %done, label \
             %again\nagain:\n  %o = call i64 @llvm.aarch64.ldxr.p0(ptr elementtype(i{bits}) \
             %p)\n  %n = add i64 %o, 1\n  %s = call i32 @llvm.aarch64.stxr.p0(i64 %n, ptr \
             elementtype(i{bits}) %p)\n  %ok = icmp eq i32 %s, 0\n  br i1 %ok, label %next, \
             label %again\nnext:\n  %i1 = add i64 %i, 1\n  br label %outer\ndone:\n  \
             %r = load i64, ptr %p\n  ret i64 %r\n}}"
        )
        .unwrap();
        for &x in &xs {
            calls.push((format!("inc{bits}"), [x, 5, 0, 0, 0, 0]));
            writeln!(want, "{}", (x & !m) | (x.wrapping_add(5) & m)).unwrap();
        }
    }
    // Pairs: written by `stxp`, read back by `ldaxp` in order, swapped by
    // `stlxp`.
    s.push_str(
        "define i64 @pair(i64 %a, i64 %b) {\nentry:\n  %p = getelementptr i8, ptr @cell, i64 0\n  \
         %hp = getelementptr i8, ptr %p, i64 8\n  store i64 0, ptr %p\n  store i64 0, ptr %hp\n  \
         br label %w\nw:\n  %o = call { i64, i64 } @llvm.aarch64.ldxp(ptr %p)\n  \
         %s = call i32 @llvm.aarch64.stxp(i64 %a, i64 %b, ptr %p)\n  %ok = icmp eq i32 %s, 0\n  \
         br i1 %ok, label %r, label %w\nr:\n  %q = call { i64, i64 } @llvm.aarch64.ldaxp(ptr %p)\n  \
         %x = extractvalue { i64, i64 } %q, 0\n  %y = extractvalue { i64, i64 } %q, 1\n  \
         %s2 = call i32 @llvm.aarch64.stlxp(i64 %y, i64 %x, ptr %p)\n  %ok2 = icmp eq i32 %s2, 0\n  \
         br i1 %ok2, label %d, label %r\nd:\n  %lo = load i64, ptr %p\n  %hi = load i64, ptr %hp\n  \
         %y3 = mul i64 %y, 3\n  %lo5 = mul i64 %lo, 5\n  %hi7 = mul i64 %hi, 7\n  \
         %t1 = add i64 %x, %y3\n  %t2 = add i64 %t1, %lo5\n  %t3 = add i64 %t2, %hi7\n  ret i64 %t3\n}\n",
    );
    for (a, b) in [(1u64, 2u64), (u64::MAX, 0x1234_5678), (0x8000_0000_0000_0000, 77)] {
        calls.push(("pair".to_owned(), [a, b, 0, 0, 0, 0]));
        let v = a.wrapping_add(b.wrapping_mul(3)).wrapping_add(b.wrapping_mul(5));
        writeln!(want, "{}", v.wrapping_add(a.wrapping_mul(7))).unwrap();
    }
    // `clrex` between a load-exclusive and a store-exclusive: the store
    // fails (status 1) and memory keeps its value.
    s.push_str(
        "define i64 @cleared(i64 %x) {\n  %p = getelementptr i8, ptr @cell, i64 0\n  store i64 %x, ptr %p\n  \
         %v = call i64 @llvm.aarch64.ldxr.p0(ptr elementtype(i64) %p)\n  \
         call void @llvm.aarch64.clrex()\n  %n = add i64 %v, 1\n  \
         %s = call i32 @llvm.aarch64.stxr.p0(i64 %n, ptr elementtype(i64) %p)\n  \
         %l = load i64, ptr %p\n  %se = zext i32 %s to i64\n  %sh = shl i64 %se, 32\n  \
         %eq = icmp eq i64 %l, %x\n  %e = zext i1 %eq to i64\n  %r = or i64 %sh, %e\n  ret i64 %r\n}\n\
         define i64 @cleared_pair(i64 %a) {\n  %p = getelementptr i8, ptr @cell, i64 0\n  \
         %hp = getelementptr i8, ptr %p, i64 8\n  store i64 %a, ptr %p\n  store i64 %a, ptr %hp\n  \
         %o = call { i64, i64 } @llvm.aarch64.ldaxp(ptr %p)\n  call void @llvm.aarch64.clrex()\n  \
         %s = call i32 @llvm.aarch64.stlxp(i64 1, i64 2, ptr %p)\n  %lo = load i64, ptr %p\n  \
         %hi = load i64, ptr %hp\n  %t = xor i64 %lo, %hi\n  %z = icmp eq i64 %t, 0\n  \
         %k = icmp eq i64 %lo, %a\n  %both = and i1 %z, %k\n  %e = zext i1 %both to i64\n  \
         %se = zext i32 %s to i64\n  %sh = shl i64 %se, 32\n  %r = or i64 %sh, %e\n  ret i64 %r\n}\n",
    );
    for x in [0u64, 41, u64::MAX] {
        calls.push(("cleared".to_owned(), [x, 0, 0, 0, 0, 0]));
        writeln!(want, "{}", (1u64 << 32) | 1).unwrap();
        calls.push(("cleared_pair".to_owned(), [x, 0, 0, 0, 0, 0]));
        writeln!(want, "{}", (1u64 << 32) | 1).unwrap();
    }
    (s, calls, want)
}

// §5.6.39: the intrinsics run as their definitions say, built by hod in
// every mode and by `llc`, under every linker.
#[test]
fn intrinsics_run_as_specified() {
    let (body, calls, want) = table();
    let count = native::run_table("a64-exclusive", &body, &calls, &MODES, Some(&want));
    eprintln!("{count} calls agree");
}

/// The attempts each `framed` call may make.
const ATTEMPTS: u64 = 1000;

/// Retry loops whose target is in the function's own frame, bounded to
/// `ATTEMPTS` attempts. Each returns 1 when it stopped within the bound
/// and memory holds what the last status says (the new value after a
/// success, the old one after failures), else 0. Whether a store-exclusive
/// ever succeeds is NOT asserted: at -O0 the function's own stores to its
/// frame between the pair (hod's, and `llc`'s for what it spills) may hit
/// the reservation granule of the target, and then every attempt fails on
/// hardware (§5.6.39, "The reservation granule"). Whether they hit it
/// depends on the stack address, so on the run.
fn framed_table() -> (String, Vec<native::Call>, String) {
    let mut s = String::from(DECLARE);
    let check = "done:\n  %m = load i64, ptr %p\n  %x1 = add i64 %x, 1\n  \
                 %w = select i1 %ok, i64 %x1, i64 %x\n  %good = icmp eq i64 %m, %w\n  \
                 %bounded = icmp ule i64 %i1, %n\n  %r1 = and i1 %good, %bounded\n";
    // The single form, one block per attempt.
    writeln!(
        s,
        "define i64 @framed(i64 %x, i64 %n) {{\nentry:\n  %p = alloca i64, align 16\n  \
         store i64 %x, ptr %p\n  br label %again\nagain:\n  %i = phi i64 [ 0, %entry ], \
         [ %i1, %again ]\n  %o = call i64 @llvm.aarch64.ldaxr.p0(ptr elementtype(i64) %p)\n  \
         %o1 = add i64 %o, 1\n  %s = call i32 @llvm.aarch64.stlxr.p0(i64 %o1, ptr \
         elementtype(i64) %p)\n  %i1 = add i64 %i, 1\n  %ok = icmp eq i32 %s, 0\n  \
         %more = icmp ult i64 %i1, %n\n  %fail = xor i1 %ok, true\n  %go = and i1 %fail, \
         %more\n  br i1 %go, label %again, label %done\n{check}  %e = zext i1 %r1 to i64\n  \
         ret i64 %e\n}}"
    )
    .unwrap();
    // The pair form; the high half must never change.
    writeln!(
        s,
        "define i64 @framed_pair(i64 %x, i64 %n) {{\nentry:\n  %p = alloca [2 x i64], align \
         16\n  %hp = getelementptr i8, ptr %p, i64 8\n  store i64 %x, ptr %p\n  store i64 %x, \
         ptr %hp\n  br label %again\nagain:\n  %i = phi i64 [ 0, %entry ], [ %i1, %again ]\n  \
         %q = call {{ i64, i64 }} @llvm.aarch64.ldaxp(ptr %p)\n  %lo = extractvalue {{ i64, \
         i64 }} %q, 0\n  %hi = extractvalue {{ i64, i64 }} %q, 1\n  %lo1 = add i64 %lo, 1\n  \
         %s = call i32 @llvm.aarch64.stlxp(i64 %lo1, i64 %hi, ptr %p)\n  %i1 = add i64 %i, 1\n  \
         %ok = icmp eq i32 %s, 0\n  %more = icmp ult i64 %i1, %n\n  %fail = xor i1 %ok, true\n  \
         %go = and i1 %fail, %more\n  br i1 %go, label %again, label %done\n{check}  \
         %h = load i64, ptr %hp\n  %hk = icmp eq i64 %h, %x\n  %r2 = and i1 %r1, %hk\n  \
         %e = zext i1 %r2 to i64\n  ret i64 %e\n}}"
    )
    .unwrap();
    let mut calls: Vec<native::Call> = Vec::new();
    let mut want = String::new();
    for name in ["framed", "framed_pair"] {
        for x in [0u64, 41, u64::MAX] {
            calls.push((name.to_owned(), [x, ATTEMPTS, 0, 0, 0, 0]));
            want.push_str("1\n");
        }
    }
    (s, calls, want)
}

// §5.6.39: a retry loop whose target is in its own frame stops within its
// bound and leaves memory as its status says, built by hod in every mode
// and by `llc`, under every linker; success is not asserted.
#[test]
fn frame_target_is_bounded() {
    let (body, calls, want) = framed_table();
    let count = native::run_table("a64-exclusive-framed", &body, &calls, &MODES, Some(&want));
    eprintln!("{count} calls agree");
}

const THREADS: u32 = 4;
const PER_THREAD: u64 = 20_000;

/// The counting workers: `count<bits>(ptr, n)` adds 1 `n` times through
/// an `ldxr`/`stxr` retry loop of one block; `count_pair(ptr, n)` adds 1
/// and 2 to the halves of a pair through `ldaxp`/`stlxp`.
fn workers() -> String {
    let mut s = String::from(DECLARE);
    for bits in BITS {
        writeln!(
            s,
            "define void @count{bits}(ptr %p, i64 %n) {{\nentry:\n  br label %outer\nouter:\n  \
             %i = phi i64 [ 0, %entry ], [ %i1, %next ]\n  %end = icmp eq i64 %i, %n\n  \
             br i1 %end, label %done, label %again\nagain:\n  %o = call i64 \
             @llvm.aarch64.ldxr.p0(ptr elementtype(i{bits}) %p)\n  %v = add i64 %o, 1\n  \
             %s = call i32 @llvm.aarch64.stxr.p0(i64 %v, ptr elementtype(i{bits}) %p)\n  \
             %ok = icmp eq i32 %s, 0\n  br i1 %ok, label %next, label %again\nnext:\n  \
             %i1 = add i64 %i, 1\n  br label %outer\ndone:\n  ret void\n}}"
        )
        .unwrap();
    }
    s.push_str(
        "define void @count_pair(ptr %p, i64 %n) {\nentry:\n  br label %outer\nouter:\n  \
         %i = phi i64 [ 0, %entry ], [ %i1, %next ]\n  %end = icmp eq i64 %i, %n\n  \
         br i1 %end, label %done, label %again\nagain:\n  \
         %q = call { i64, i64 } @llvm.aarch64.ldaxp(ptr %p)\n  \
         %x = extractvalue { i64, i64 } %q, 0\n  %y = extractvalue { i64, i64 } %q, 1\n  \
         %x1 = add i64 %x, 1\n  %y1 = add i64 %y, 2\n  \
         %s = call i32 @llvm.aarch64.stlxp(i64 %x1, i64 %y1, ptr %p)\n  \
         %ok = icmp eq i32 %s, 0\n  br i1 %ok, label %next, label %again\nnext:\n  \
         %i1 = add i64 %i, 1\n  br label %outer\ndone:\n  ret void\n}\n",
    );
    s
}

fn driver() -> String {
    format!(
        "#include <pthread.h>\n#include <stdint.h>\n#include <stdio.h>\n\
         void count8(void *, uint64_t); void count16(void *, uint64_t);\n\
         void count32(void *, uint64_t); void count64(void *, uint64_t);\n\
         void count_pair(void *, uint64_t);\n\
         static _Alignas(64) uint8_t c8; static _Alignas(64) uint16_t c16;\n\
         static _Alignas(64) uint32_t c32; static _Alignas(64) uint64_t c64;\n\
         static _Alignas(64) uint64_t cp[2];\n\
         static void *work(void *arg) {{\n  (void)arg;\n  for (int r = 0; r < 4; r++) {{\n    \
         count8(&c8, {PER_THREAD} / 4); count16(&c16, {PER_THREAD} / 4);\n    \
         count32(&c32, {PER_THREAD} / 4); count64(&c64, {PER_THREAD} / 4);\n    \
         count_pair(cp, {PER_THREAD} / 4);\n  }}\n  return 0;\n}}\n\
         int main(void) {{\n  pthread_t t[{THREADS}];\n  \
         for (int i = 0; i < {THREADS}; i++) pthread_create(&t[i], 0, work, 0);\n  \
         for (int i = 0; i < {THREADS}; i++) pthread_join(t[i], 0);\n  \
         printf(\"%u %u %u %llu %llu %llu\\n\", c8, c16, c32, (unsigned long long)c64,\n         \
         (unsigned long long)cp[0], (unsigned long long)cp[1]);\n  return 0;\n}}\n"
    )
}

// §5.6.39: four threads count through exclusive retry loops of one block,
// built by hod plain and stressed and by `llc`; every total is exact.
#[test]
fn threads_count_exactly() {
    let dir = Scratch::new("a64-exclusive-threads");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", &driver(), &["-pthread"]);
    let text = format!("{}{}", ll_header(22), workers());
    let (module, bodies) = parse(&text);
    let mut objects: Vec<(String, PathBuf)> = Vec::new();
    for (mode, options) in &MODES[..2] {
        let emitted =
            compile_module(&module, &bodies, options).unwrap_or_else(|e| panic!("{mode}: {e}"));
        objects.push((format!("hod-{mode}"), write_object(&dir, &format!("{mode}.o"), &emitted)));
    }
    if let Some(object) = llc(&dir, "llc", &text, &[]) {
        objects.push(("llc".to_owned(), object));
    }
    let total = u64::from(THREADS) * PER_THREAD;
    let want =
        format!("{} {} {} {total} {total} {}\n", total % 256, total % 65_536, total, 2 * total);
    let qemu = qemu_on_aarch64();
    let mut runs = 0;
    for (name, object) in &objects {
        for linker in &toolchain.linkers {
            let program = dir.join(format!("{name}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &["-pthread"], &[&driver, object], &program);
            assert!(ran.success(), "{name} {}: {}", linker.name, ran.stderr_text());
            assert_eq!(ran.stdout_text(), want, "{name} {}", linker.name);
            runs += 1;
        }
        if let (Some(qemu), Some(linker)) = (&qemu, toolchain.linkers.first()) {
            let program = dir.join(format!("{name}-{}", linker.name));
            let ran = under_qemu(qemu, &program);
            assert_eq!(ran, want, "{name} under qemu");
            runs += 1;
        }
    }
    eprintln!("{runs} runs of {THREADS} threads, every total exact");
}

/// `qemu-aarch64` on an AArch64 host (elsewhere the toolchain runs every
/// program under it already).
fn qemu_on_aarch64() -> Option<PathBuf> {
    if cfg!(target_arch = "aarch64") { find_tool("qemu-aarch64") } else { None }
}

/// The output of `program` under `qemu-aarch64 -cpu cortex-a53`.
fn under_qemu(qemu: &Path, program: &Path) -> String {
    let mut command = Command::new(qemu);
    command.args(["-cpu", hod_testutil::target::AARCH64_EMULATED_CPU]).arg(program);
    let ran = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(ran.success(), "{} under qemu: {}", program.display(), ran.stderr_text());
    ran.stdout_text()
}

// §5.6.39: the table again under `qemu-aarch64 -cpu cortex-a53` on an
// AArch64 host, hod's plain build and `llc`'s, so that what the hosted
// x86-64 lane runs under the emulator is checked here too.
// The bounded loops with their target in the frame run there too.
#[test]
fn intrinsics_run_under_qemu() {
    let Some(qemu) = qemu_on_aarch64() else { return };
    let (body, calls, want) = table();
    let n = table_under_qemu(&qemu, "a64-exclusive-qemu", &body, &calls, &want);
    let (body, calls, want) = framed_table();
    let m = table_under_qemu(&qemu, "a64-exclusive-qemu-framed", &body, &calls, &want);
    eprintln!("{n} and {m} calls (times builds) agree under qemu");
}

/// Builds `body` with hod plain and with `llc`, links it with a driver
/// that prints each call's result and runs it under qemu against `want`.
/// Returns the number of builds checked (0 without the tools).
fn table_under_qemu(
    qemu: &Path,
    label: &str,
    body: &str,
    calls: &[native::Call],
    want: &str,
) -> usize {
    let dir = Scratch::new(label);
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return 0 };
    let mut c = String::from("#include <stdio.h>\n#include <stdint.h>\n");
    let mut declared: Vec<&str> = Vec::new();
    for (name, _) in calls {
        if !declared.contains(&name.as_str()) {
            writeln!(c, "uint64_t {name}(uint64_t, uint64_t);").unwrap();
            declared.push(name);
        }
    }
    c.push_str("int main(void) {\n");
    for (name, args) in calls {
        writeln!(
            c,
            "  printf(\"%llu\\n\", (unsigned long long){name}({}ULL, {}ULL));",
            args[0], args[1]
        )
        .unwrap();
    }
    c.push_str("  return 0;\n}\n");
    let driver = toolchain.compile_c(&dir, "driver", &c, &[]);
    let text = format!("{}{body}", ll_header(22));
    let (module, bodies) = parse(&text);
    let emitted = compile_module(&module, &bodies, &DEFAULT).unwrap_or_else(|e| panic!("{e}"));
    let mut objects = vec![("hod", write_object(&dir, "hod.o", &emitted))];
    if let Some(object) = llc(&dir, "llc", &text, &[]) {
        objects.push(("llc", object));
    }
    let Some(linker) = toolchain.linkers.first() else { return 0 };
    for (name, object) in &objects {
        let program = dir.join(format!("{name}-{}", linker.name));
        toolchain.link_ok(linker, &[], &[&driver, object], &program);
        assert_eq!(under_qemu(qemu, &program), want, "{label} {name} under qemu");
    }
    calls.len() * objects.len()
}

// §5.6.39: what hod cannot read is refused by name: a missing
// `elementtype` (LLVM's verifier rejects it; hod's reader keeps the call),
// one that is not `i8`..`i64` (`llc` aborts), and calls whose types are
// not the intrinsic's.
#[test]
fn the_rest_is_refused_by_name() {
    let load = |attr: &str| {
        format!(
            "declare i64 @llvm.aarch64.ldxr.p0(ptr)\ndefine i64 @f(ptr %p) {{\n  %v = call i64 \
             @llvm.aarch64.ldxr.p0(ptr {attr} %p)\n  ret i64 %v\n}}\n"
        )
    };
    let store = |attr: &str| {
        format!(
            "declare i32 @llvm.aarch64.stxr.p0(i64, ptr)\ndefine i32 @f(ptr %p) {{\n  %s = call i32 \
             @llvm.aarch64.stxr.p0(i64 1, ptr {attr} %p)\n  ret i32 %s\n}}\n"
        )
    };
    let mut cases: Vec<(String, &'static str)> = Vec::new();
    for attr in
        ["", "elementtype(i1)", "elementtype(i24)", "elementtype(i128)", "elementtype(float)"]
    {
        cases.push((load(attr), ELEMENT));
        cases.push((store(attr), ELEMENT));
    }
    cases.push((load("elementtype(ptr)"), ELEMENT));
    cases.push((
        "declare i32 @llvm.aarch64.ldxr.p0(ptr)\ndefine i32 @f(ptr %p) {\n  %v = call i32 \
         @llvm.aarch64.ldxr.p0(ptr elementtype(i32) %p)\n  ret i32 %v\n}\n"
            .to_owned(),
        TYPE,
    ));
    cases.push((
        "declare i32 @llvm.aarch64.stxr.p0(i32, ptr)\ndefine i32 @f(ptr %p) {\n  %s = call i32 \
         @llvm.aarch64.stxr.p0(i32 1, ptr elementtype(i32) %p)\n  ret i32 %s\n}\n"
            .to_owned(),
        TYPE,
    ));
    cases.push((
        "declare { i32, i32 } @llvm.aarch64.ldxp(ptr)\ndefine { i32, i32 } @f(ptr %p) {\n  \
         %v = call { i32, i32 } @llvm.aarch64.ldxp(ptr %p)\n  ret { i32, i32 } %v\n}\n"
            .to_owned(),
        TYPE,
    ));
    let cases: Vec<(&str, &'static str)> = cases.iter().map(|(t, r)| (t.as_str(), *r)).collect();
    assert_refused(&cases);
}
