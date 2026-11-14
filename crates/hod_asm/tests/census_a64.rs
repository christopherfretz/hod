//! `spec/asm.md` §16: the census of the AArch64 inline-assembly templates
//! real code writes (WP-216), each body as the template reads once its
//! operands are substituted the way `llc` prints them (`r` operands are
//! `x8`, `x9`, ... here). Every body the assembler accepts encodes to the
//! words `llvm-mc` 22 and 19 write for the same text; every body it
//! refuses is refused by the name the table gives. The record
//! (`spec/landed/asm/WP-216.md`) has the table and how it was taken.

use std::fmt::Write as _;

use hod_asm::a64::{Code, Features, assemble};
use hod_testutil::{Scratch, run, tool};

/// What hod does with a census body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Want {
    /// Assembled.
    Ok,
    /// Refused by this name.
    No(&'static str),
}

use Want::{No, Ok};

/// One census row: the substituted body, where it is written, the number
/// of templates of that shape the census found, and what hod does.
struct Row {
    body: &'static str,
    from: &'static str,
    count: u32,
    want: Want,
}

const fn row(body: &'static str, from: &'static str, count: u32, want: Want) -> Row {
    Row { body, from, count, want }
}

/// The census, most frequent shape first within each group.
const CENSUS: &[Row] = &[
    // CRC (zlib's `crc32.c`, vendored by several projects).
    row("crc32x w8, w8, x9", "zlib crc32.c (boost, opencv, mold copies)", 28, Ok),
    row("crc32b w8, w8, w9", "zlib crc32.c", 8, Ok),
    // Spin-wait hints and barriers.
    row("nop", "boost fiber cpu_relax, atomic, tbb", 21, Ok),
    row("yield", "boost core/interprocess, tbb, abseil", 9, Ok),
    row("isb", "boost atomic thread_pause", 3, Ok),
    row("isb sy", "tbb _machine.h", 1, Ok),
    row("dmb ish", "boost atomic fence_arch_ops_gcc_aarch64", 1, Ok),
    row("dmb ishld", "boost atomic fence_arch_ops_gcc_aarch64", 1, Ok),
    // System registers.
    row("mrs x8, cntvct_el0", "benchmark cycleclock.h, abseil, highway, opencv", 11, Ok),
    row("mrs x8, cntfrq_el0", "abseil unscaledcycleclock.cc, highway", 3, Ok),
    row("mrs x8, MIDR_EL1", "abseil crc cpu_detect.cc (5 calls in kiln-built modules)", 2, Ok),
    row("mrs x8, tpidr_el0", "mimalloc prim-tls.h", 1, Ok),
    row("mrs x8, tpidrro_el0\nbic x8, x8, #7", "mimalloc prim-tls.h", 1, Ok),
    row("mrs\tx8, fpcr", "glibc fpu_control.h _FPU_GETCW", 1, Ok),
    row("msr\tfpcr, x8", "glibc fpu_control.h _FPU_SETCW", 1, Ok),
    row("mrs\tx8, fpsr", "glibc fpu_control.h _FPU_GETFPSR", 1, Ok),
    row("msr\tfpsr, x8", "glibc fpu_control.h _FPU_SETFPSR", 1, Ok),
    // Bit manipulation.
    row("rbit x8, x9", "simdjson arm64 bitmanipulation.h", 5, Ok),
    row("clz x8, x9", "upb encode.c (protobuf php, grpc copies)", 4, Ok),
    row("umaddl x8, w9, w10, x11", "xxhash.h (zstd copies)", 3, Ok),
    row("str x9, [x8], #4", "libjpeg-turbo jchuff.h", 1, Ok),
    // Traps and pointer authentication.
    row("brk #0", "doctest.h, psnip debug-trap", 3, Ok),
    row("hint #0x7;", "abseil addresses.h (4 calls in kiln-built modules)", 1, Ok),
    row("xpaclri", "abseil addresses.h (the commented alternative)", 1, Ok),
    // Prefetch (§16).
    row(
        "PRFM PLDL1KEEP, [x8,1024]",
        "rocksdb crc32c_arm64.h (16 calls in the kiln-built module)",
        4,
        Ok,
    ),
    row("prfm pldl1keep, [x8]", "eigen NEON PacketMath.h, zstd compiler.h (Q operand)", 3, Ok),
    row("prfm pldl2keep, [x8]", "zstd compiler.h (Q operand)", 2, Ok),
    // A computed branch into the template, past `.p2align 3` (§17; the
    // census of WP-216 left out the directive, written under
    // `#ifndef __SEH__` inside the string).
    row(
        "adr x8, 0f\nadd x8, x8, x10, lsl #3\nmov w9, #0x80\nbr x8\n.p2align 3\n0:\n\
         orr x8, x9, x11, lsr #56\nstrb w8, [x12, #8]\norr x8, x9, x11, lsr #49\n\
         strb w8, [x12, #7]\norr w8, w9, w11, lsr #21\nstrb w8, [x12, #3]\n\
         orr w8, w11, #0x80\nstrb w8, [x12]",
        "upb encode.c (protobuf php, grpc copies)",
        4,
        Ok,
    ),
    // valgrind.h's client requests and hidden calls (arm64).
    row(
        "mov x3, x9\nmov x4, x10\nror x12, x12, #3  ;  ror x12, x12, #13 \n\t\
         ror x12, x12, #51 ;  ror x12, x12, #61 \n\torr x10, x10, x10\nmov x8, x3",
        "valgrind.h VALGRIND_DO_CLIENT_REQUEST_EXPR",
        1,
        Ok,
    ),
    row(
        "ror x12, x12, #3  ;  ror x12, x12, #13 \n\tror x12, x12, #51 ;  ror x12, x12, #61 \n\t\
         orr x11, x11, x11\nmov x8, x3",
        "valgrind.h VALGRIND_GET_NR_CONTEXT",
        1,
        Ok,
    ),
    row(
        "mov x21, sp\nbic sp, x21, #15\nldr x0, [x8, #8]\nldr x8, [x8]\n\
         ror x12, x12, #3  ;  ror x12, x12, #13 \n\tror x12, x12, #51 ;  ror x12, x12, #61 \n\t\
         orr x12, x12, x12\nmov sp,  x21\nmov x8, x0",
        "valgrind.h CALL_FN_W_* (13 arities)",
        13,
        Ok,
    ),
    // Scalar SIMD forms (§17).
    row("fcvtzs d8, d9", "highway arm_neon-inl.h (scalar SIMD form)", 1, Ok),
    row("fcvtzu d8, d9", "highway arm_neon-inl.h (scalar SIMD form)", 1, Ok),
    row("fcmeq d8, d9, d9", "highway arm_neon-inl.h (scalar SIMD form)", 1, Ok),
    // zstd's 34 `.p2align 3..6` templates (`zstd_lazy.c`,
    // `zstd_decompress_block.c`, cmake's and mold's copies), counted
    // here by WP-216, are all under `#if defined(__GNUC__) &&
    // defined(__x86_64__)`: x86-64 templates, `census_x64.rs` (§17).
    // Advanced SIMD on vector operands (§20). The `4h`/`8h` forms need
    // `fullfp16`, `pmull`/`pmull2` of `1d`/`2d` need `aes`: the census
    // bodies are assembled with every feature, as the projects' functions
    // that hold them are compiled.
    row("fmla v8.4s, v9.4s, v10.s[0]", "eigen NEON GeneralBlockPanelKernel.h", 8, Ok),
    row("fcvtzs v8.4s, v9.4s", "highway arm_neon-inl.h (and .2d, .4h, .8h)", 5, Ok),
    row("fcvtzu v8.4s, v9.4s", "highway arm_neon-inl.h (and .2d, .4h, .8h)", 5, Ok),
    row("fcmeq v8.8h, v9.8h, v9.8h", "highway arm_neon-inl.h", 5, Ok),
    row("fcvtns v8.4s, v9.4s", "opencv intrin_neon.hpp", 1, Ok),
    row("umlal v8.2d, v9.2s, v10.2s", "xxhash.h (zstd copies)", 3, Ok),
    row(
        "pmull v8.1q, v9.1d, v10.1d",
        "abseil crc32_x86_arm_combined_simd.h (and grpc's copy)",
        2,
        Ok,
    ),
    row(
        "pmull2 v8.1q, v9.2d, v10.2d",
        "abseil crc32_x86_arm_combined_simd.h (and grpc's copy)",
        2,
        Ok,
    ),
    // Stays refused.
    row(
        ".arch_extension  crypto",
        "abseil randen_hwaes.cc (grpc's bloaty copy)",
        1,
        No("asm:directive/arch_extension"),
    ),
    row("cntb x8", "highway targets.cc (SVE)", 1, No("asm:mnemonic/cntb")),
    row(".inst 0xd4200000", "psnip debug-trap (opencv)", 1, No("asm:directive/inst")),
];

const ALL: Features = Features::ALL;

fn hod(body: &str) -> Result<Vec<u32>, hod_asm::Refusal> {
    let mut code = Code::default();
    assemble(body, ALL, &mut code, &mut String::new())?;
    assert!(code.refs.is_empty(), "{body}: no symbol references expected");
    Result::Ok(code.words)
}

#[test]
fn census_bodies_are_as_the_table_says() {
    let mut report = String::new();
    let (mut taken, mut all) = (0, 0);
    for r in CENSUS {
        let got = match hod(r.body) {
            Result::Ok(_) => Ok,
            Err(e) => No(e.0),
        };
        if got != r.want {
            writeln!(report, "  {:?} ({}): hod {got:?}, table {:?}", r.body, r.from, r.want)
                .unwrap();
        }
        all += r.count;
        if r.want == Ok {
            taken += r.count;
        }
    }
    assert!(report.is_empty(), "the census table disagrees:\n{report}");
    println!("census_a64: {taken} of {all} templates assembled");
}

/// The words of `.text` of `source` as `llvm-mc-<major>` assembles it.
fn llvm(major: u32, source: &str, scratch: &Scratch) -> Option<Vec<u32>> {
    let mc = tool(&format!("llvm-mc-{major}"))?;
    let objcopy = tool(&format!("llvm-objcopy-{major}"))?;
    let src = scratch.write(&format!("census-{major}.s"), source);
    let obj = scratch.join(format!("census-{major}.o"));
    let bin = scratch.join(format!("census-{major}.bin"));
    let out = run(
        &mc,
        [
            "--triple=aarch64-unknown-linux-gnu".as_ref(),
            "-mattr=+lse,+crc,+fp-armv8,+aes,+fullfp16".as_ref(),
            "-filetype=obj".as_ref(),
            "-o".as_ref(),
            obj.as_os_str(),
            src.as_os_str(),
        ],
    );
    assert!(out.success(), "llvm-mc-{major} refused the census bodies:\n{}", out.stderr_text());
    let out = run(
        &objcopy,
        [
            "-O".as_ref(),
            "binary".as_ref(),
            "--only-section=.text".as_ref(),
            obj.as_os_str(),
            bin.as_os_str(),
        ],
    );
    assert!(out.success(), "llvm-objcopy failed: {}", out.stderr_text());
    let bytes = std::fs::read(&bin).expect("objcopy output");
    Some(bytes.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

#[test]
fn accepted_census_bodies_are_llvm_mc_words() {
    let mut ours = Vec::new();
    let mut spans = Vec::new();
    let mut source = String::new();
    for r in CENSUS.iter().filter(|r| r.want == Ok) {
        let words = hod(r.body).unwrap_or_else(|e| panic!("{:?} refused: {e}", r.body));
        spans.push((r.body, ours.len(), words.len()));
        ours.extend(words);
        source.push_str(r.body);
        source.push('\n');
    }
    let scratch = Scratch::new("hod-asm-census-a64");
    for major in [22, 19] {
        let Some(theirs) = llvm(major, &source, &scratch) else { continue };
        let mut report = String::new();
        for &(body, at, len) in &spans {
            let (a, b) = (&ours[at..at + len], theirs.get(at..at + len));
            if Some(a) != b {
                writeln!(report, "  {body:?}: hod {a:08x?}, llvm {b:08x?}").unwrap();
            }
        }
        assert!(
            report.is_empty() && ours.len() == theirs.len(),
            "LLVM {major} disagrees ({} vs {} words):\n{report}",
            ours.len(),
            theirs.len()
        );
        println!(
            "census_a64: {} bodies, {} words equal llvm-mc-{major}'s",
            spans.len(),
            ours.len()
        );
    }
}
